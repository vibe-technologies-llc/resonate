#!/bin/sh
set -e

if [ "$1" = configure ] && [ -d /run/systemd/system ]; then
    systemctl --system daemon-reload >/dev/null || true
    if [ -n "${2:-}" ]; then
        deb-systemd-invoke try-restart resonate-server.service >/dev/null || true
    fi
fi
