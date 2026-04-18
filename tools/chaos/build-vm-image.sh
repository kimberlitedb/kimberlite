#!/usr/bin/env bash
# Builds the VM images used by `kimberlite-chaos run <scenario> --apply`.
#
# Runs on the EPYC Hetzner host (or any Linux box with qemu-img, nbd, tar,
# mke2fs, and root access). Produces under /opt/kimberlite-dst/vm-images/:
#
#   bzImage                        — the Linux kernel for the guest VMs
#   base.qcow2                     — master rootfs image (Debian + kimberlite)
#   replica-c0-r0.qcow2            — clone for cluster 0 replica 0
#   replica-c0-r1.qcow2            — clone for cluster 0 replica 1
#   replica-c0-r2.qcow2            — clone for cluster 0 replica 2
#
# The clones are byte-for-byte identical: per-replica identity is supplied at
# boot time via kernel cmdline (see chaos_controller.rs). Cloning via `cp` is
# fine because qcow2 is copy-on-write.
#
# Invoke via `just epyc-build-vm-image`. Idempotent: re-running regenerates
# everything without manual cleanup.
#
# This is the REAL-BINARY variant — runs kimberlite-server inside the VMs
# with VSR consensus + the chaos HTTP probe surface. Replaces the earlier
# musl-shim approach; kimberlite-server does NOT actually pull DuckDB at
# runtime, so stock glibc + x86_64-unknown-linux-gnu is all we need.

set -euo pipefail

# --- config --------------------------------------------------------------
readonly DEBIAN_VERSION="${DEBIAN_VERSION:-trixie}"
readonly DEBIAN_ARCH="${DEBIAN_ARCH:-amd64}"
readonly DEBIAN_MIRROR="${DEBIAN_MIRROR:-http://deb.debian.org/debian}"
readonly VM_IMAGE_DIR="${VM_IMAGE_DIR:-/opt/kimberlite-dst/vm-images}"
readonly BUILD_DIR="${BUILD_DIR:-/opt/kimberlite-dst/vm-build}"
readonly REPO_DIR="${REPO_DIR:-/opt/kimberlite-dst/repo}"
readonly NUM_REPLICAS="${NUM_REPLICAS:-3}"
# Number of clusters to materialize images for. Multi-cluster scenarios
# (independent_cluster_isolation) need cluster 0 AND 1. Keep at 2 so we
# always have spare images; cheap via cp --reflink.
readonly NUM_CLUSTERS="${NUM_CLUSTERS:-2}"
# Debian-slim rootfs needs more headroom than Alpine did — glibc is chunky.
readonly DISK_SIZE="${DISK_SIZE:-1G}"

# --- helpers -------------------------------------------------------------
log() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
die() { log "ERROR: $*" >&2; exit 1; }

cleanup_nbd() {
  if [[ -n "${NBD_DEV:-}" ]] && [[ -e "${NBD_DEV}" ]]; then
    umount "${ROOTFS_MOUNT}" 2>/dev/null || true
    qemu-nbd --disconnect "${NBD_DEV}" 2>/dev/null || true
  fi
}
trap cleanup_nbd EXIT

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

# --- preflight -----------------------------------------------------------
[[ "$(id -u)" -eq 0 ]] || die "must run as root (needs nbd + mount + debootstrap)"
require_cmd qemu-img
require_cmd qemu-nbd
require_cmd mkfs.ext4
require_cmd tar
require_cmd debootstrap
require_cmd ldd

log "loading nbd kernel module"
modprobe nbd max_part=8 || die "failed to load nbd module"

mkdir -p "${VM_IMAGE_DIR}" "${BUILD_DIR}"
readonly ROOTFS_DIR="${BUILD_DIR}/rootfs"
readonly ROOTFS_MOUNT="${BUILD_DIR}/mnt"
rm -rf "${ROOTFS_DIR}" && mkdir -p "${ROOTFS_DIR}" "${ROOTFS_MOUNT}"

# --- 1. debootstrap minimal Debian rootfs ---------------------------------
# `--variant=minbase` keeps the rootfs ~150 MiB — enough for glibc, busybox
# (as coreutils), iproute2, ca-certificates, but no systemd, no apt cache.
log "bootstrapping Debian ${DEBIAN_VERSION} (${DEBIAN_ARCH}) into ${ROOTFS_DIR}"
debootstrap \
  --variant=minbase \
  --arch="${DEBIAN_ARCH}" \
  --include="libc6,libgcc-s1,libstdc++6,ca-certificates,iproute2,busybox" \
  "${DEBIAN_VERSION}" \
  "${ROOTFS_DIR}" \
  "${DEBIAN_MIRROR}" \
  || die "debootstrap failed"

# --- 2. copy kimberlite binary --------------------------------------------
# The production kimberlite CLI bundles kimberlite-server. Build the release
# binary with the default x86_64-unknown-linux-gnu target — no musl
# cross-compile gymnastics needed. DuckDB is a workspace dev-dependency for
# SQL differential testing; it does NOT enter the kimberlite-server
# dependency tree. (`cargo tree -p kimberlite-server` to verify.)
readonly KMB_BIN="${REPO_DIR}/target/release/kimberlite"
if [[ ! -x "${KMB_BIN}" ]]; then
  die "kimberlite release binary not found at ${KMB_BIN}
Run this first (on EPYC):
    cd ${REPO_DIR}
    cargo build --release -p kimberlite-cli"
fi

log "installing kimberlite binary"
install -D -m 0755 "${KMB_BIN}" "${ROOTFS_DIR}/usr/local/bin/kimberlite"

# Copy all dynamic libraries the binary needs. glibc is already in the
# rootfs via debootstrap, but `ldd` catches any stragglers (aws-lc,
# libgcc_s, libstdc++, librustls, etc. — some linked dynamically depending
# on Rust toolchain + cargo features).
log "auditing dynamic library dependencies (ldd)"
readonly LDD_OUTPUT=$(ldd "${KMB_BIN}" || true)
echo "${LDD_OUTPUT}" | while IFS= read -r line; do
  # Extract the resolved path: `libfoo.so.1 => /lib/x86_64-linux-gnu/libfoo.so.1 (0x...)`
  resolved=$(echo "${line}" | awk '{print $3}')
  if [[ -n "${resolved}" ]] && [[ "${resolved}" != "(0x"* ]] && [[ -f "${resolved}" ]]; then
    # Mirror the library into the rootfs at the same path.
    dest="${ROOTFS_DIR}${resolved}"
    if [[ ! -f "${dest}" ]]; then
      mkdir -p "$(dirname "${dest}")"
      cp -L "${resolved}" "${dest}"
    fi
  fi
done

# --- 3. install minimal /sbin/init ----------------------------------------
# Debian-slim has systemd, but we don't want it for chaos VMs: slower boot,
# more attack surface, extra failure modes. Replace /sbin/init with a tiny
# shell script that parses /proc/cmdline, sets up networking, initialises
# the data dir, and execs kimberlite.
log "installing /sbin/init shim"
rm -f "${ROOTFS_DIR}/sbin/init"
cat >"${ROOTFS_DIR}/sbin/init" <<'EOF'
#!/bin/sh
# PID 1 for chaos VMs. Mount pseudo-filesystems, parse cmdline, configure
# networking, initialise data dir, exec kimberlite server in cluster mode.

set +e

mount -t proc none /proc 2>/dev/null
mount -t sysfs none /sys 2>/dev/null
mount -t devtmpfs none /dev 2>/dev/null
mount -t tmpfs none /tmp 2>/dev/null
mount -t tmpfs none /run 2>/dev/null

# Parse /proc/cmdline for kmb.* params. Ubuntu/Debian kernels have
# CONFIG_IP_PNP=n, so we carry per-replica IP in our own kmb.ip=/kmb.gw=
# parameters which we plumb into `ip addr add` + `ip route add default`.
for param in $(cat /proc/cmdline); do
    case "$param" in
        kmb.replica_id=*)      export KMB_REPLICA_ID="${param#kmb.replica_id=}" ;;
        kmb.bind=*)            export KMB_BIND_ADDR="${param#kmb.bind=}" ;;
        kmb.http_bind=*)       KMB_HTTP_BIND="${param#kmb.http_bind=}" ;;
        kmb.own=*)             export KMB_OWN_ADDR="${param#kmb.own=}" ;;
        kmb.cluster_peers=*)   export KMB_CLUSTER_PEERS="${param#kmb.cluster_peers=}" ;;
        kmb.ip=*)              KMB_IP="${param#kmb.ip=}" ;;
        kmb.gw=*)              KMB_GW="${param#kmb.gw=}" ;;
    esac
done

# Derive HTTP port from kmb.http_bind=0.0.0.0:9000 for kimberlite's
# KMB_HTTP_PORT env var.
if [ -n "$KMB_HTTP_BIND" ]; then
    export KMB_HTTP_PORT="${KMB_HTTP_BIND##*:}"
fi

# Turn on the chaos HTTP probe surface inside the binary.
export KMB_ENABLE_CHAOS_ENDPOINTS=1

echo "[init] kmb.replica_id=$KMB_REPLICA_ID bind=$KMB_BIND_ADDR ip=$KMB_IP gw=$KMB_GW http_port=$KMB_HTTP_PORT" >/dev/console

ip link set dev lo up 2>/dev/console
ip link set dev eth0 up 2>/dev/console
if [ -n "$KMB_IP" ]; then
    ip addr add "$KMB_IP" dev eth0 2>/dev/console
fi
if [ -n "$KMB_GW" ]; then
    ip route add default via "$KMB_GW" 2>/dev/console
fi
ip addr show eth0 2>/dev/console

echo "[init] starting kimberlite server (cluster mode)..." >/dev/console

# Persistent data dir on the ext4 root volume so VSR's superblock +
# write log survive reboots (kill+restart chaos scenarios).
mkdir -p /var/lib/kimberlite 2>/dev/null
if [ ! -f /var/lib/kimberlite/.kimberlite-initialized ]; then
    /usr/local/bin/kimberlite init /var/lib/kimberlite --yes 2>/dev/console
    : >/var/lib/kimberlite/.kimberlite-initialized
fi

# `kimberlite start --cluster` reads KMB_REPLICA_ID + KMB_CLUSTER_PEERS and
# binds the HTTP sidecar on KMB_HTTP_PORT (default 9000). Exec so the
# server runs as PID 1 and receives signals directly.
exec /usr/local/bin/kimberlite start --cluster /var/lib/kimberlite --address "$KMB_BIND_ADDR"
EOF
chmod 0755 "${ROOTFS_DIR}/sbin/init"

cat >"${ROOTFS_DIR}/etc/issue" <<'EOF'
kimberlite-chaos VM (\l) — real binary
EOF

# --- 4. use host kernel (virtio_blk + ext4 built in, no initramfs needed) ---
readonly HOST_VMLINUZ=$(ls -1 /boot/vmlinuz-[0-9]* 2>/dev/null | sort -V | tail -1)
[[ -f "${HOST_VMLINUZ}" ]] || die "no /boot/vmlinuz-* on host"
log "using host kernel ${HOST_VMLINUZ}"
install -m 0644 "${HOST_VMLINUZ}" "${VM_IMAGE_DIR}/bzImage"

# --- 5. build the base qcow2 ---------------------------------------------
readonly BASE_IMG="${VM_IMAGE_DIR}/base.qcow2"
log "creating base qcow2 (${DISK_SIZE}) at ${BASE_IMG}"
rm -f "${BASE_IMG}"
qemu-img create -f qcow2 "${BASE_IMG}" "${DISK_SIZE}"

# Find a free nbd device.
NBD_DEV=""
for i in 0 1 2 3 4 5 6 7; do
  if ! grep -q . /sys/block/nbd${i}/size 2>/dev/null || [[ "$(cat /sys/block/nbd${i}/size)" == "0" ]]; then
    NBD_DEV="/dev/nbd${i}"
    break
  fi
done
[[ -n "${NBD_DEV}" ]] || die "no free /dev/nbd device"

log "connecting ${BASE_IMG} to ${NBD_DEV}"
qemu-nbd --connect="${NBD_DEV}" "${BASE_IMG}"
sleep 1

log "formatting ${NBD_DEV} as ext4"
mkfs.ext4 -q -F -L kimberlite "${NBD_DEV}"

mount "${NBD_DEV}" "${ROOTFS_MOUNT}"
log "copying rootfs into ${NBD_DEV}"
tar -C "${ROOTFS_DIR}" -cf - . | tar -C "${ROOTFS_MOUNT}" -xf -
sync
umount "${ROOTFS_MOUNT}"
qemu-nbd --disconnect "${NBD_DEV}"
NBD_DEV=""

# --- 6. clone per-cluster per-replica qcow2 ------------------------------
for c in $(seq 0 $((NUM_CLUSTERS - 1))); do
  for r in $(seq 0 $((NUM_REPLICAS - 1))); do
    dst="${VM_IMAGE_DIR}/replica-c${c}-r${r}.qcow2"
    log "cloning to ${dst}"
    cp --reflink=auto "${BASE_IMG}" "${dst}"
  done
done

log "done. Images under ${VM_IMAGE_DIR}:"
ls -lh "${VM_IMAGE_DIR}"
