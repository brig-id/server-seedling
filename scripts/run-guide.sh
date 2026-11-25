#!/usr/bin/env bash
# Follows docs/install.md on this machine, as a person would: a fresh Linux
# with systemd and sudo (the guide job of CI, on the Ubuntu runner itself).
# Every block the guide marks `run` is run in order, as one script, and its
# file is written; then what the guide promises is checked, up to a person
# signed in on the site it registered.
#
#   scripts/run-guide.sh <install_guide test executable> [<seedling binary>]
#
# With a binary, step 1 downloads that one, from a server on this machine,
# with a SHA256SUMS made as the release makes it; without, the latest
# release (on a tag, once it is published).
#
# What the guide leaves to the reader, given here: the domain points to this
# machine (/etc/hosts), and the reverse proxy is installed (Caddy, with its
# own local authority: no public one reaches a CI machine).
set -euo pipefail

tests="$(realpath "$1")"
binary="${2:+$(realpath "$2")}"
domain=id.example.org
work="$(mktemp -d)"
script="$work/guide.sh"

files=""
finish() {
  status=$?
  [ -n "$files" ] && kill "$files" 2> /dev/null
  if [ "$status" != 0 ]; then
    echo "::group::What the server and the proxy said"
    sudo journalctl -u seedling -u caddy -n 200 --no-pager || true
    echo "::endgroup::"
  fi
}
trap finish EXIT

echo "::group::The machine: $domain is here, and a reverse proxy"
echo "127.0.0.1 $domain" | sudo tee -a /etc/hosts > /dev/null
sudo apt-get update -qq
sudo apt-get install -y -qq caddy
# Caddy's local authority, trusted by this machine: what a public one is to
# the machine of a reader. The guide's Caddyfile then gets its certificate
# from it (below).
sudo tee /etc/caddy/Caddyfile > /dev/null << 'EOF'
{
	local_certs
}
localhost {
	respond "ok"
}
EOF
sudo systemctl restart caddy
root=/var/lib/caddy/.local/share/caddy/pki/authorities/local/root.crt
for _ in $(seq 1 30); do sudo test -f "$root" && break; sleep 1; done
sudo cp "$root" /usr/local/share/ca-certificates/caddy-local.crt
sudo update-ca-certificates > /dev/null
echo "::endgroup::"

if [ -n "$binary" ]; then
  echo "::group::The download: this build, as the release publishes it"
  mkdir "$work/release"
  cp "$binary" "$work/release/seedling-linux-$(uname -m)"
  chmod 755 "$work/release/seedling-linux-$(uname -m)"
  (cd "$work/release" && sha256sum seedling-* > SHA256SUMS && cat SHA256SUMS)
  python3 -m http.server 8000 --bind 127.0.0.1 --directory "$work/release" \
    > "$work/files.log" 2>&1 &
  files=$!
  for _ in $(seq 1 30); do curl -fsS -o /dev/null http://127.0.0.1:8000/SHA256SUMS && break; sleep 1; done
  echo "::endgroup::"
fi

# -- The guide, as a script ---------------------------------------------------

GUIDE_SCRIPT="$script" "$tests" --ignored --exact write_the_guide_as_a_script > /dev/null

# What differs from a reader's machine, each replaced where the guide has it.
replace() { # <exact text> <replacement>
  if ! grep -qF -- "$1" "$script"; then
    echo "The guide no longer has \`$1\`: change scripts/run-guide.sh with it."
    exit 1
  fi
  FROM="$1" TO="$2" perl -0pi -e 's/\Q$ENV{FROM}\E/$ENV{TO}/g' "$script"
}
if [ -n "$binary" ]; then
  replace "URL=https://github.com/brig-id/server-seedling/releases/latest/download" \
    "URL=http://127.0.0.1:8000"
fi
# The certificate from the local authority, not from Let's Encrypt.
replace "$domain {" $'{\n\tlocal_certs\n}\n'"$domain {"
# A person waits a few seconds after a start; a script, not.
replace "curl -fsS https://" "curl -fsS --retry 20 --retry-delay 1 --retry-all-errors https://"
# Followed for a few seconds, then stopped as a person would.
replace "sudo journalctl -u seedling -f " "{ timeout 5 sudo journalctl -u seedling -f || test \$? = 124; } "

echo "::group::The script"
cat "$script"
echo "::endgroup::"

bash "$script" 2>&1 | tee "$work/guide.log"

# -- What the guide promises --------------------------------------------------

echo "-- The outcome"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | sed -n 1p)
if [ -n "$binary" ]; then
  test "$(seedling --version)" = "seedling $version"
fi
systemctl is-active --quiet seedling
echo "✓ the service runs: $(seedling --version)"
curl -fsS "https://$domain/health" | jq -e '.status == "ok"' > /dev/null
echo "✓ https://$domain/health answers"
sudo test -f /etc/brigid/master.key.cred
sudo test ! -e /etc/brigid/master.key
echo "✓ the key is sealed, and the clear one gone"
sudo journalctl -u seedling --no-pager | grep -qF "https://$domain/setup/#link="
grep -qF "https://$domain/setup/#link=" "$work/guide.log"
echo "✓ the setup link of the first administrator is printed, at start and by admin link"
curl -fsS "https://$domain/auth/server" | jq -e '.policy == "open"' > /dev/null
echo "✓ anyone may sign up"
sudo seedling client list | grep -q "Field Notes"
client_id=$(sed -n 's/^client_id=//p' "$work/guide.log" | tail -n 1)
client_secret=$(sed -n 's/^client_secret=//p' "$work/guide.log" | tail -n 1)
test -n "$client_id" && test -n "$client_secret"
echo "✓ the site is registered: $client_id"

# A new link: every start of the guide's later steps replaced the earlier one.
setup=$(sudo seedling admin link 2> /dev/null | grep -o "https://$domain/setup/#link=[^[:space:]]*" | tail -n 1)
test -n "$setup"
GUIDE_SETUP_ADDRESS="$setup" GUIDE_CLIENT_ID="$client_id" GUIDE_CLIENT_SECRET="$client_secret" \
  "$tests" --ignored --exact a_person_signs_in_on_the_site_the_guide_registered --nocapture
