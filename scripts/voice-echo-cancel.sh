#!/usr/bin/env bash
# Echo cancellation for the buddy's voice using PipeWire's own module (ADR-0015 item 5).
#   voice-echo-cancel.sh try [SECONDS]   load it for a while (default 60 s) and check the nodes, nothing permanent
#   voice-echo-cancel.sh enable    install the drop-in permanently and restart the user's PipeWire
#   voice-echo-cancel.sh disable   remove the drop-in
# After `enable`, put these two lines in ~/.config/octoplus/voice.conf and restart octoplus-voice:
#   capture_node = octoplus_echo_source
#   playback_node = octoplus_echo_sink
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
conf="$root/deploy/pipewire/octoplus-echo-cancel.conf"
dest="$HOME/.config/pipewire/pipewire.conf.d/octoplus-echo-cancel.conf"
nodes() { pw-cli ls Node 2>/dev/null | grep -c "octoplus_echo_" || true; }
case "${1:-}" in
  try)
    # A module loaded from pw-cli lives only as long as that process: hold it for N seconds.
    secs="${2:-60}"
    {
      echo 'load-module libpipewire-module-echo-cancel { library.name = aec/libspa-aec-webrtc node.latency = 1024/48000 monitor.mode = false source.props = { node.name = octoplus_echo_source media.class = Audio/Source } sink.props = { node.name = octoplus_echo_sink media.class = Audio/Sink } }'
      sleep "$secs"
    } | pw-cli >/dev/null 2>&1 &
    holder=$!
    sleep 3
    echo "echo-cancel nodes in the graph: $(nodes)"
    pw-cli ls Node | grep -E "octoplus_echo_(source|sink)" || { echo "FAIL: nodes missing"; kill "$holder" 2>/dev/null || true; exit 1; }
    echo "loaded for $secs s (for this session only)"
    wait "$holder" || true ;;
  enable)
    install -d "$(dirname "$dest")"; install -m 0644 "$conf" "$dest"
    systemctl --user restart pipewire.service pipewire-pulse.service wireplumber.service 2>/dev/null || true
    sleep 3; echo "echo-cancel nodes in the graph: $(nodes)" ;;
  disable)
    rm -f "$dest"; systemctl --user restart pipewire.service pipewire-pulse.service wireplumber.service 2>/dev/null || true
    echo "removed" ;;
  *) echo "usage: $0 try|enable|disable"; exit 2 ;;
esac
