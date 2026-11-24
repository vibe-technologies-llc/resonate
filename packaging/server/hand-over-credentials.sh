#!/bin/sh
set -eu
umask 077

from=${RESONATE_SERVER_CREDENTIALS_FROM:-/etc/resonate-server}
runtime=${RESONATE_SERVER_RUNTIME_DIRECTORY:-/run/resonate-server}
to=${RESONATE_SERVER_CREDENTIALS_TO:-/run/resonate-server-credentials}

if [ "${1:-}" = withdraw ]; then
    rm -rf "$to"
    exit 0
fi

group=$(stat -c %g "$runtime")

install -d -m 0750 -g "$group" "$to"
install -m 0440 -g "$group" "$from/fullchain.pem" "$to/certificate.handed"
install -m 0440 -g "$group" "$from/privkey.pem" "$to/key.handed"
mv -f "$to/certificate.handed" "$to/certificate"
mv -f "$to/key.handed" "$to/key"

if [ $# -gt 0 ]; then
    kill -HUP "$1"
fi
