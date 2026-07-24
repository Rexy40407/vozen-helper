#!/usr/bin/env bash
set -euo pipefail

version="${1:?release version required}"
root=/home/vozen/vozen-helper-rust
node_root=/home/vozen/vozen-helper
release="$root/releases/$version"

mkdir -p "$release" "$root/shared/data"
tar -xzf /home/vozen/helper-release.tgz -C "$release"
cp "$node_root/.env" "$root/shared/.env"

sed -i \
  -e '/^DISCORD_APPLICATION_ID=/d' \
  -e '/^DISCORD_OAUTH_CLIENT_ID=/d' \
  -e '/^DISCORD_OAUTH_CLIENT_SECRET=/d' \
  -e '/^DISCORD_OAUTH_REDIRECT_URI=/d' \
  -e '/^HELPER_DATABASE_URL=/d' \
  -e '/^HELPER_BIND_ADDR=/d' \
  -e '/^HELPER_SESSION_SECRET=/d' \
  -e '/^HELPER_ALLOWED_ORIGIN=/d' \
  -e '/^HELPER_API_ONLY=/d' \
  "$root/shared/.env"

client_id="$(awk -F= '/^CLIENT_ID=/{print $2}' "$node_root/.env")"
printf '\nDISCORD_APPLICATION_ID=%s\n' "$client_id" >> "$root/shared/.env"
printf 'DISCORD_OAUTH_CLIENT_ID=%s\n' "$client_id" >> "$root/shared/.env"
printf 'DISCORD_OAUTH_CLIENT_SECRET=%s\n' "$(openssl rand -hex 32)" >> "$root/shared/.env"
printf 'DISCORD_OAUTH_REDIRECT_URI=https://helper.vozen.org/oauth/callback\n' >> "$root/shared/.env"
printf 'HELPER_DATABASE_URL=%s/vozen-helper.db\n' "$node_root" >> "$root/shared/.env"
printf 'HELPER_BIND_ADDR=127.0.0.1:8788\n' >> "$root/shared/.env"
printf 'HELPER_SESSION_SECRET=%s\n' "$(openssl rand -hex 32)" >> "$root/shared/.env"
printf 'HELPER_ALLOWED_ORIGIN=https://helper.vozen.org\nHELPER_API_ONLY=false\n' >> "$root/shared/.env"
chmod 600 "$root/shared/.env"
chmod 755 "$release/bin/vozen-helper"
ln -sfn "$release" "$root/current"

stat -c '%n %s bytes' "$release/bin/vozen-helper"
