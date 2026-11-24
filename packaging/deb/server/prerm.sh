#!/bin/sh
set -e

if [ "$1" = remove ] && [ -d /run/systemd/system ]; then
    deb-systemd-invoke stop resonate-server.service >/dev/null || true
fi
