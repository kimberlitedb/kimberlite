#!/sbin/openrc-run
# OpenRC service: start kimberlite with per-replica identity pulled from
# /proc/cmdline. The chaos controller bakes the following parameters into
# each VM's kernel cmdline:
#
#   kmb.replica_id=<N>
#   kmb.bind=<ip:port>
#   kmb.peers=<ip:port>,<ip:port>,...
#
# The Linux kernel's built-in ip= parameter handles the network config,
# so by the time this runs eth0 is already up.

name="kimberlite"
description="Kimberlite chaos shim (HTTP probe responder)"
command="/usr/local/bin/kimberlite-chaos-shim"
pidfile="/run/kimberlite.pid"
start_stop_daemon_args="--background --make-pidfile"

depend() {
    need net
    after firewall
}

_kmb_arg() {
    local key="$1"
    tr ' ' '\n' </proc/cmdline | sed -n "s/^${key}=\(.*\)/\1/p" | head -1
}

start_pre() {
    local replica_id bind_addr peers
    replica_id="$(_kmb_arg kmb.replica_id)"
    bind_addr="$(_kmb_arg kmb.bind)"
    peers="$(_kmb_arg kmb.peers)"

    [ -n "${bind_addr}" ] || bind_addr="0.0.0.0:9000"
    [ -n "${replica_id}" ] || replica_id="0"

    ebegin "kimberlite replica_id=${replica_id} bind=${bind_addr} peers=${peers}"

    mkdir -p /var/lib/kimberlite

    # Expose as env vars for any future config that wants them.
    export KMB_REPLICA_ID="${replica_id}"
    export KMB_PEERS="${peers}"
    export KMB_BIND_ADDR="${bind_addr}"

    # The shim reads everything from env, no positional args needed.
    command_args=""

    eend 0
}
