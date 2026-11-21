# Installing seedling

From a machine of your own to a person signed in on a website, with brig·id
as the way in. About half an hour.

You need:

- **A Linux machine** with systemd. x86-64 or arm64; any distribution: the
  executable links nothing.
- **A domain name** for the server, such as `id.example.org`, whose DNS
  record points to that machine. It cannot change later: passkeys are tied to
  it.
- **A certificate for that name**: from the reverse proxy you already run, or
  from one you add (step 4), or files you give the server itself. A
  certificate obtained by seedling itself is coming.

Every command below is typed on that machine.

## 1. Download and check

This downloads the latest release for your machine's architecture, checks
it against the sums published with it, installs it, and leaves nothing
behind:

```sh
URL=https://github.com/brig-id/server-seedling/releases/latest/download
FILE=seedling-linux-$(uname -m)
TMP=$(mktemp -d)
curl -fsSL -o "$TMP/$FILE" "$URL/$FILE"
curl -fsSL "$URL/SHA256SUMS" | grep " $FILE\$" | (cd "$TMP" && sha256sum --check)
sudo install -m 755 "$TMP/$FILE" /usr/local/bin/seedling
rm -rf "$TMP"
seedling --version
```

With the GitHub command line you can also check that the file was built by
this repository's workflow, from its source: run
`gh attestation verify "$TMP/$FILE" --repo brig-id/server-seedling` before
the `rm`.

## 2. Prepare the machine

```sh
sudo seedling init --domain id.example.org --systemd
```

The server listens on this machine only, on port 8080; the proxy of step 4
forwards to it. If something else uses 8080 here (`ss -ltn` lists the ports
in use), choose another: `--port 8091`. `init` warns when the port is taken.

It writes, and never overwrites:

| File | What it is |
| --- | --- |
| `/etc/brigid/seedling.toml` | The configuration: the domain, and where the server listens |
| `/etc/brigid/master.key` | The master key, readable by root only |
| `/etc/systemd/system/seedling.service` | The service: it runs as the user `brigid`, never as root |
| `/etc/sysusers.d/brigid.conf` | That user |

**The master key.** Everything the database holds is encrypted with it.
The key itself is a file readable by root only, in clear: the server needs
to read it at each start, unattended. systemd reads it and hands it to the
service alone; the user `brigid` cannot read the file. What this protects:
a copy of the database, a backup, a disk image of `/var/lib/brigid`. What
it does not: someone who is root on the machine, or who takes the whole
disk. For the second, keep the key on another disk:
`--no-key-file --key-file /mnt/keys/master.key`, and put it there yourself.

**Or seal the key with the machine.** On a machine with a TPM (most PCs and
mini PCs from the last years; `systemd-analyze has-tpm2` says `yes`), the key
can be sealed instead of written in clear:

```sh
sudo seedling init --domain id.example.org --systemd --seal
```

The key is then `/etc/brigid/master.key.cred`, encrypted by `systemd-creds`
with the TPM: systemd unseals it for the service, and a command typed as root
on the machine unseals it the same way. A copy of the disk, or the disk
itself, is useless without this machine. `init` prints the key once, on its
last line: that copy, kept offline, is the only way back if the machine or its
TPM is replaced.

| The key… | Protects a copy of the database, a backup | Protects the disk taken away |
| --- | --- | --- |
| in clear, root only (the default) | yes | no |
| on another disk (`--no-key-file --key-file …`) | yes | yes, if that disk stays |
| sealed by the TPM (`--seal`) | yes | yes |

None of them protects against someone who is root on the running machine.

A server installed with a clear key seals it afterwards, the server running:

```sh
sudo seedling seal-key            # seals /etc/brigid/master.key, switches the unit
sudo systemctl daemon-reload && sudo systemctl restart seedling
sudo journalctl -u seedling -n 20 --no-pager
sudo shred -u /etc/brigid/master.key   # once it runs, and the key is kept offline
```

On a new machine, from the offline copy: `sudo seedling seal-key --from <file>`.

**Keep a copy of the key somewhere else, offline.** Without it the database
cannot be read, by anyone, you included.

## 3. Start it

```sh
sudo systemd-sysusers
sudo systemctl daemon-reload
sudo systemctl enable --now seedling
sudo journalctl -u seedling -n 20 --no-pager
```

The last command shows what the server said at its start, and with it the
setup link of step 5. `seedling init` prints these same four commands.

## 4. The certificate and the proxy

People reach the server at `https://id.example.org`. Whatever answers there
holds the certificate and forwards every request to the server, on this
machine, at the port of step 2. It must:

- **forward everything**, on one name: the server's pages and its API share
  the origin;
- keep the `Host` the browser sent;
- **replace** `X-Forwarded-For` with the client's address. The server
  limits how fast each client may try, by that address. A proxy that
  *appends* to the header lets a client choose the address it is counted
  as.

If you already run a reverse proxy, give it a site with these three
properties. Two examples:

**Caddy**, which also obtains and renews the certificate by itself, in
`/etc/caddy/Caddyfile` (Caddy replaces `X-Forwarded-For` by default):

```
id.example.org {
	reverse_proxy 127.0.0.1:8080
}
```

**nginx**, with a certificate you already have:

```
server {
    listen 443 ssl;
    server_name id.example.org;
    ssl_certificate     /etc/ssl/id.example.org/fullchain.pem;
    ssl_certificate_key /etc/ssl/id.example.org/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        # Replaced, not appended ($proxy_add_x_forwarded_for would append).
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto https;
    }
}
```

**No proxy at all**: give the server the certificate itself. In
`/etc/brigid/seedling.toml`, set `host = "0.0.0.0"`, `port = 443`,
`tls_cert` and `tls_key` (files the user `brigid` can read), and
`trust_forwarded_for = false`: nothing in front sets the header, so a
client could. Then `sudo systemctl restart seedling`.

Whichever you chose:

```sh
curl -fsS https://id.example.org/health
```

## 5. The first administrator

A new server has no account. At each start it writes a one-time link that
creates the first one, which administers the server. It is in what step 3
showed; again:

```sh
sudo journalctl -u seedling -n 20 --no-pager
```

Open the address `https://id.example.org/setup/#link=…` it shows, within the
hour: the last one, if it shows several (each start makes a new one and
cancels the one before). Expired, or not there? This
prints a new one, with no restart (the earlier one stops working):

```sh
sudo seedling admin link
```

Then create the account with your passkey. Keep the recovery code
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

**Most websites need nothing from you.** A website that describes itself
publishes a small document (for example
`https://notes.example.org/oauth-client-metadata.json`) and gives its address
as its `client_id`: your server reads it and signs your people in to that
website, with no registration and no secret to hand over. The person sees the
website's domain and decides. Which of those websites your server accepts:

```sh
sudo seedling client policy              # what it is now: open, unless you changed it
sudo seedling client policy open         # any, except the ones you refuse (the default)
sudo seedling client policy listed       # only the ones you accept
sudo seedling client policy registered   # none: only the websites you register below
sudo seedling client accept https://notes.example.org/oauth-client-metadata.json
sudo seedling client refuse https://phishing.example/oauth-client-metadata.json
sudo seedling client unlist https://notes.example.org/oauth-client-metadata.json
```

An accepted website's logo is shown when people sign in to it; any other
website is shown by its domain and the name it declares.

**A website of your own, or one under development on your machine**, can be
registered instead, in **Administration → Applications**, or on the machine:

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

## Updating

The same download as step 1, then a restart. The database is upgraded in
place at start; the configuration, the key and the service are kept:

```sh
URL=https://github.com/brig-id/server-seedling/releases/latest/download
FILE=seedling-linux-$(uname -m)
TMP=$(mktemp -d)
curl -fsSL -o "$TMP/$FILE" "$URL/$FILE"
curl -fsSL "$URL/SHA256SUMS" | grep " $FILE\$" | (cd "$TMP" && sha256sum --check)
sudo install -m 755 "$TMP/$FILE" /usr/local/bin/seedling
rm -rf "$TMP"
sudo systemctl restart seedling
seedling --version
```

What changed in each release:
[the releases page](https://github.com/brig-id/server-seedling/releases).
The **About** page of the server says which version runs.

## Day to day

```sh
sudo seedling --version                 # what runs
sudo seedling client list               # the applications: registered, and those that describe themselves
sudo seedling admin link                # a new setup link; or, once accounts exist, an administrator again
sudo journalctl -u seedling -f          # what it says
```

**Back up**: `/var/lib/brigid/brigid.db`, with the server running
(`sudo sqlite3 /var/lib/brigid/brigid.db ".backup /somewhere/brigid.db"`),
and the master key, kept apart from it.

A command typed as root on the machine reads the key, then works as the user
`brigid`: the database never gets a file root owns. The server itself refuses
to run as root.
