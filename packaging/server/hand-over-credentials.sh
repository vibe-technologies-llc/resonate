#!/bin/sh
set -eu

from=${RESONATE_SERVER_CREDENTIALS_FROM:-/etc/resonate-server}
to=${RESONATE_SERVER_CREDENTIALS_TO:-/run/resonate-server}
owner=$(stat -c %u "$to")

install -m 0400 -o "$owner" "$from/fullchain.pem" "$to/certificate.handed"
install -m 0400 -o "$owner" "$from/privkey.pem" "$to/key.handed"
mv -f "$to/certificate.handed" "$to/certificate"
mv -f "$to/key.handed" "$to/key"

if [ $# -gt 0 ]; then
    kill -HUP "$1"
fi
