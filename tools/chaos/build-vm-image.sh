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
require_cmd cargo

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

# --- 2. copy kimberlite musl binary --------------------------------------
readonly KIMBERLITE_BIN="${REPO_DIR}/target/x86_64-unknown-linux-musl/release/kimberlite"
if [[ ! -x "${KIMBERLITE_BIN}" ]]; then
  die "static kimberlite binary not found at ${KIMBERLITE_BIN}
Run this first (on EPYC):
    rustup target add x86_64-unknown-linux-musl
    cd ${REPO_DIR}
    cargo build --release --target x86_64-unknown-linux-musl -p kimberlite-cli"
fi

log "installing kimberlite binary"
install -D -m 0755 "${KIMBERLITE_BIN}" "${ROOTFS_DIR}/usr/local/bin/kimberlite"

# --- 3. install OpenRC service + boot scripts ----------------------------
log "installing OpenRC service"
install -D -m 0755 "${REPO_DIR}/tools/chaos/init-kimberlite.sh" \
  "${ROOTFS_DIR}/etc/init.d/kimberlite"

# Enable the service at boot (default runlevel).
mkdir -p "${ROOTFS_DIR}/etc/runlevels/default"
ln -sf /etc/init.d/kimberlite "${ROOTFS_DIR}/etc/runlevels/default/kimberlite"

# Also enable the kernel's built-in ip= config so the network comes up before
# our service runs.
mkdir -p "${ROOTFS_DIR}/etc/network"
cat >"${ROOTFS_DIR}/etc/network/interfaces" <<'EOF'
auto lo
iface lo inet loopback

# eth0 is configured by the kernel's ip= cmdline parameter.
auto eth0
iface eth0 inet manual
EOF

# Keep an empty data dir. The OpenRC script will populate it on boot.
mkdir -p "${ROOTFS_DIR}/var/lib/kimberlite"

# Useful defaults.
cat >"${ROOTFS_DIR}/etc/issue" <<'EOF'
kimberlite-chaos VM (\l)
EOF

# --- 4. grab the Alpine virt kernel --------------------------------------
readonly APK_CACHE="${BUILD_DIR}/apk-cache"
mkdir -p "${APK_CACHE}"
readonly KERNEL_APK_URL="${ALPINE_MIRROR}/v${ALPINE_VERSION}/main/${ALPINE_ARCH}"
log "fetching Alpine virt kernel index"
curl -fsSL "${KERNEL_APK_URL}/" -o "${APK_CACHE}/index.html" || true
readonly KERNEL_APK=$(grep -oE 'linux-virt-[0-9][^"]*\.apk' "${APK_CACHE}/index.html" \
  | sort -V | tail -1)
[[ -n "${KERNEL_APK}" ]] || die "could not find linux-virt-*.apk in Alpine index"
log "fetching ${KERNEL_APK}"
curl -fsSL "${KERNEL_APK_URL}/${KERNEL_APK}" -o "${APK_CACHE}/${KERNEL_APK}"
rm -rf "${APK_CACHE}/kernel" && mkdir -p "${APK_CACHE}/kernel"
tar -xzf "${APK_CACHE}/${KERNEL_APK}" -C "${APK_CACHE}/kernel" || true

# Copy the vmlinuz out.
readonly VMLINUZ=$(find "${APK_CACHE}/kernel/boot" -name 'vmlinuz-virt' -print -quit)
[[ -f "${VMLINUZ}" ]] || die "vmlinuz-virt not found in kernel apk"
install -m 0644 "${VMLINUZ}" "${VM_IMAGE_DIR}/bzImage"
log "installed ${VM_IMAGE_DIR}/bzImage"

# Copy the matching modules into the rootfs so drivers load at boot.
readonly MOD_DIR=$(find "${APK_CACHE}/kernel/lib/modules" -maxdepth 1 -mindepth 1 -type d -print -quit)
if [[ -n "${MOD_DIR}" ]]; then
  mkdir -p "${ROOTFS_DIR}/lib/modules"
  cp -a "${MOD_DIR}" "${ROOTFS_DIR}/lib/modules/"
fi

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
