# Installing seedling

From a machine of your own to a person signed in on a website, with brig·id
as the way in. About half an hour.

You need:

- **A Linux machine** with systemd, reachable from the Internet on ports 80
  and 443. x86-64 or arm64; any distribution: the executable links nothing.
- **A domain name** for the server, such as `id.example.org`, whose DNS
  record points to that machine. It cannot change later: passkeys are tied to
  it.
- **A reverse proxy** that obtains the certificate (this guide uses Caddy,
  which does it by itself). A certificate built into seedling is coming.

Every command below is typed on that machine.

## 1. Download and check

Take the latest release from
[the releases page](https://github.com/brig-id/server-seedling/releases):
its version, and your machine's architecture (`uname -m` says `x86_64` or
`aarch64`).

```sh
VERSION=v0.7.0
ARCH=$(uname -m)
curl -fLO "https://github.com/brig-id/server-seedling/releases/download/$VERSION/seedling-$VERSION-linux-$ARCH"
curl -fLO "https://github.com/brig-id/server-seedling/releases/download/$VERSION/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS
sudo install -m 755 "seedling-$VERSION-linux-$ARCH" /usr/local/bin/seedling
seedling --version
```

With the GitHub command line, you can also check that the file was built by
this repository's workflow, from its source:

```sh
gh attestation verify "seedling-$VERSION-linux-$ARCH" --repo brig-id/server-seedling
```

## 2. Prepare the machine

```sh
sudo seedling init --domain id.example.org --systemd
```

It writes, and never overwrites:

| File | What it is |
| --- | --- |
| `/etc/brigid/seedling.toml` | The configuration: the domain, a server listening on `127.0.0.1:8080` behind the proxy |
| `/etc/brigid/master.key` | The master key, readable by root only. Everything stored is encrypted with it |
| `/etc/systemd/system/seedling.service` | The service: it runs as the user `brigid`, never as root |
| `/etc/sysusers.d/brigid.conf` | That user |

**Keep a copy of `/etc/brigid/master.key` somewhere else, offline.** Without
it, the database cannot be read, by anyone, you included. If the machine's
disk could be stolen whole, keep the key on another disk instead:
`--no-key-file --key-file /mnt/keys/master.key`, and put it there yourself.

## 3. Start it

```sh
sudo systemd-sysusers
sudo systemctl daemon-reload
sudo systemctl enable --now seedling
systemctl status seedling
```

## 4. The certificate and the proxy

Install [Caddy](https://caddyserver.com/docs/install), then give it this
site in `/etc/caddy/Caddyfile`:

```
id.example.org {
	reverse_proxy 127.0.0.1:8080
}
```

```sh
sudo systemctl reload caddy
curl -fsS https://id.example.org/health
```

## 5. The first administrator

A new server has no account. At each start it writes a one-time link that
creates the first one, which administers the server:

```sh
sudo journalctl -u seedling -n 30 --no-pager
```

Open the address `https://id.example.org/setup/#link=…` it shows, within the
hour, and create the account with your passkey. Then keep the recovery code
the onboarding gives you: nobody, an administrator included, can give access
back to an account.

From the account, **Administration** shows the accounts (never what they
hold), the applications, who may sign up, and a log.

## 6. Who may sign up

A new server takes accounts **by invitation**: make invitation links in
**Administration → Sign-up**. To let anyone create an account, choose
"Anyone" there, or on the machine:

```sh
sudo seedling signup open
```

## 7. Connect a website

Register it in **Administration → Applications**, or on the machine:

```sh
sudo seedling client add --name "Field Notes" --redirect-uri https://notes.example.org/callback --required email --optional given_name
```

It prints the application's `client_id` and, once, its `client_secret`: give
both to the website. A website that runs entirely in the browser or on a
phone cannot keep a secret: add `--public`, and it gets none.

The website then uses OpenID Connect with these settings:

| Setting | Value |
| --- | --- |
| Issuer (discovery) | `https://id.example.org` (`/.well-known/openid-configuration`) |
| Flow | Authorization Code with PKCE (`S256`), required |
| Scopes | `openid`; `profile` and `email` for what its registration lets it ask |
| ID token | EdDSA (Ed25519) |

What it receives is an identifier per identity and per website, and the
attributes the person accepted to share, never more. The demonstration site,
[demo.brig.id](https://demo.brig.id), is a working example in a few hundred
lines ([its source](https://github.com/brig-id/demo)).

## 8. Sign in

Open the website, choose to sign in: the browser goes to
`https://id.example.org`, the person uses their passkey, chooses which of
their identities the website meets and what it receives, and comes back
signed in.

## Day to day

```sh
sudo seedling --version                 # what runs
sudo seedling client list               # the registered applications
sudo seedling admin link                # an administrator again, if every administrator lost their passkeys
sudo journalctl -u seedling -f          # what it says
```

**Update**: download the new release as in step 1, then
`sudo systemctl restart seedling`. The database is upgraded in place at
start.

**Back up**: `/var/lib/brigid/brigid.db`, with the server running
(`sqlite3 /var/lib/brigid/brigid.db ".backup /somewhere/brigid.db"`), and the
master key, kept apart from it.

A command typed as root on the machine reads the key, then works as the user
`brigid`: the database never gets a file root owns. The server itself refuses
to run as root.
