#!/usr/bin/env bash
# Builds the VM images used by `kimberlite-chaos run <scenario> --apply`.
#
# Runs on the EPYC Hetzner host (or any Linux box with qemu-img, nbd, tar,
# mke2fs, and root access). Produces under /opt/kimberlite-dst/vm-images/:
#
#   bzImage                        — the Linux kernel for the guest VMs
#   base.qcow2                     — master rootfs image (Alpine + kimberlite)
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

set -euo pipefail

# --- config --------------------------------------------------------------
readonly ALPINE_VERSION="${ALPINE_VERSION:-3.19}"
readonly ALPINE_PATCH="${ALPINE_PATCH:-3.19.7}"
readonly ALPINE_ARCH="${ALPINE_ARCH:-x86_64}"
readonly ALPINE_MIRROR="${ALPINE_MIRROR:-https://dl-cdn.alpinelinux.org/alpine}"
readonly VM_IMAGE_DIR="${VM_IMAGE_DIR:-/opt/kimberlite-dst/vm-images}"
readonly BUILD_DIR="${BUILD_DIR:-/opt/kimberlite-dst/vm-build}"
readonly REPO_DIR="${REPO_DIR:-/opt/kimberlite-dst/repo}"
readonly NUM_REPLICAS="${NUM_REPLICAS:-3}"
readonly DISK_SIZE="${DISK_SIZE:-500M}"

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
[[ "$(id -u)" -eq 0 ]] || die "must run as root (needs nbd + mount)"
require_cmd qemu-img
require_cmd qemu-nbd
require_cmd mkfs.ext4
require_cmd tar
require_cmd curl

log "loading nbd kernel module"
modprobe nbd max_part=8 || die "failed to load nbd module"

mkdir -p "${VM_IMAGE_DIR}" "${BUILD_DIR}"
readonly ROOTFS_DIR="${BUILD_DIR}/rootfs"
readonly ROOTFS_MOUNT="${BUILD_DIR}/mnt"
rm -rf "${ROOTFS_DIR}" && mkdir -p "${ROOTFS_DIR}" "${ROOTFS_MOUNT}"

# --- 1. fetch Alpine minirootfs -----------------------------------------
readonly ROOTFS_TGZ="${BUILD_DIR}/alpine-minirootfs.tgz"
readonly ROOTFS_URL="${ALPINE_MIRROR}/v${ALPINE_VERSION}/releases/${ALPINE_ARCH}/alpine-minirootfs-${ALPINE_PATCH}-${ALPINE_ARCH}.tar.gz"
if [[ ! -f "${ROOTFS_TGZ}" ]]; then
  log "downloading ${ROOTFS_URL}"
  for attempt in 1 2 3; do
    if curl -fsSL "${ROOTFS_URL}" -o "${ROOTFS_TGZ}"; then
      break
    fi
    log "retry ${attempt}/3 after failure"
    sleep 5
  done
  [[ -f "${ROOTFS_TGZ}" ]] || die "failed to download Alpine minirootfs"
fi

log "extracting Alpine minirootfs to ${ROOTFS_DIR}"
tar -xzf "${ROOTFS_TGZ}" -C "${ROOTFS_DIR}"

# --- 2. copy kimberlite chaos shim binary --------------------------------
# We install kimberlite-chaos-shim (std-only, musl-static) rather than the
# full kimberlite-cli — the production binary pulls DuckDB (C++), which
# requires an x86_64-linux-musl C++ cross-compiler that Ubuntu doesn't
# ship. The shim exposes `/health` and `/kv/chaos-probe`, which is the
# exact surface area the chaos InvariantChecker probes.
readonly SHIM_BIN="${REPO_DIR}/target/x86_64-unknown-linux-musl/release/kimberlite-chaos-shim"
if [[ ! -x "${SHIM_BIN}" ]]; then
  die "static kimberlite-chaos-shim not found at ${SHIM_BIN}
Run this first (on EPYC):
    rustup target add x86_64-unknown-linux-musl
    cd ${REPO_DIR}
    cargo build --release --target x86_64-unknown-linux-musl -p kimberlite-chaos-shim"
fi

log "installing kimberlite-chaos-shim binary"
install -D -m 0755 "${SHIM_BIN}" "${ROOTFS_DIR}/usr/local/bin/kimberlite-chaos-shim"

# --- 3. install minimal /sbin/init shim ----------------------------------
# Alpine minirootfs ships busybox + libc but not OpenRC. Rather than
# chroot-install openrc, we use busybox as /init and a tiny shell script
# that reads /proc/cmdline for kmb.* params, exports them, and execs the
# chaos shim. This is the minimum viable PID 1 — no service manager, no
# setup/teardown scripts. A real Kimberlite production VM would want
# OpenRC; the chaos VM does not.
log "installing /sbin/init shim"
rm -f "${ROOTFS_DIR}/sbin/init"
cat >"${ROOTFS_DIR}/sbin/init" <<'EOF'
#!/bin/sh
# PID 1 for chaos VMs. Mount pseudo-filesystems, parse cmdline, configure
# networking, exec shim.

set +e
/bin/busybox --install -s 2>/dev/null || true

mount -t proc none /proc 2>/dev/null
mount -t sysfs none /sys 2>/dev/null
mount -t devtmpfs none /dev 2>/dev/null
mount -t tmpfs none /tmp 2>/dev/null
mount -t tmpfs none /run 2>/dev/null

# Parse /proc/cmdline for kmb.* and kmb.ip=/kmb.gw= — we carry per-replica
# IP in our own kmb.ip= because Ubuntu's kernel does NOT have CONFIG_IP_PNP
# enabled, so the standard `ip=` parameter is silently ignored.
for param in $(cat /proc/cmdline); do
    case "$param" in
        kmb.replica_id=*)  export KMB_REPLICA_ID="${param#kmb.replica_id=}" ;;
        kmb.bind=*)        export KMB_BIND_ADDR="${param#kmb.bind=}" ;;
        kmb.own=*)         export KMB_OWN_ADDR="${param#kmb.own=}" ;;
        kmb.peers=*)       export KMB_PEERS="${param#kmb.peers=}" ;;
        kmb.ip=*)          KMB_IP="${param#kmb.ip=}" ;;
        kmb.gw=*)          KMB_GW="${param#kmb.gw=}" ;;
    esac
done

echo "[init] kmb.replica_id=$KMB_REPLICA_ID bind=$KMB_BIND_ADDR ip=$KMB_IP gw=$KMB_GW" >/dev/console

ip link set dev lo up 2>/dev/console
ip link set dev eth0 up 2>/dev/console
if [ -n "$KMB_IP" ]; then
    ip addr add "$KMB_IP" dev eth0 2>/dev/console
fi
if [ -n "$KMB_GW" ]; then
    ip route add default via "$KMB_GW" 2>/dev/console
fi
ip addr show eth0 2>/dev/console

echo "[init] starting kimberlite-chaos-shim..." >/dev/console
exec /usr/local/bin/kimberlite-chaos-shim
EOF
chmod 0755 "${ROOTFS_DIR}/sbin/init"

# Useful defaults.
cat >"${ROOTFS_DIR}/etc/issue" <<'EOF'
kimberlite-chaos VM (\l)
EOF

# --- 4. use host kernel (virtio_blk + ext4 built in, no initramfs needed) ---
# Alpine's linux-virt kernel ships virtio_blk as a module, so direct-kernel-
# boot against an ext4 qcow2 panics on "VFS: Unable to mount root fs" unless
# we also build an initramfs with the relevant modules. Rather than chroot
# into Alpine to run mkinitfs, we use the host's Ubuntu kernel which has
# CONFIG_VIRTIO_BLK=y and CONFIG_EXT4_FS=y. All the guest needs is a
# flat ext4 rootfs visible on /dev/vda — which is exactly what we have.
readonly HOST_VMLINUZ=$(ls -1 /boot/vmlinuz-[0-9]* 2>/dev/null | sort -V | tail -1)
[[ -f "${HOST_VMLINUZ}" ]] || die "no /boot/vmlinuz-* on host"
log "using host kernel ${HOST_VMLINUZ}"
install -m 0644 "${HOST_VMLINUZ}" "${VM_IMAGE_DIR}/bzImage"

# The host kernel is standalone — no modules from the kernel apk land in the
# guest rootfs. That's fine for a chaos VM: virtio_blk + virtio_net + ext4
# are all built in, and the shim's only userspace dep is libc (already in
# the Alpine rootfs).

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

# --- 6. clone per-replica qcow2 ------------------------------------------
for r in $(seq 0 $((NUM_REPLICAS - 1))); do
  dst="${VM_IMAGE_DIR}/replica-c0-r${r}.qcow2"
  log "cloning to ${dst}"
  cp --reflink=auto "${BASE_IMG}" "${dst}"
done

log "done. Images under ${VM_IMAGE_DIR}:"
ls -lh "${VM_IMAGE_DIR}"
