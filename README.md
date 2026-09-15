# quantframe-server

A headless, self-hosted server version of [Quantframe](https://github.com/Kenya-DK/quantframe-react) by Kenya-DK,
for one user on a home network. Forked from quantframe-react at commit `3d59c4e7` (v1.6.28).

Licensed under GPLv3, like the upstream project.

Design: `docs/superpowers/specs/2026-09-14-quantframe-server-design.md`.

## Running on the homelab

Docker runs on the server only.

```bash
# copy the repo to the server, e.g. ~/stacks/quantframe-server, then on the server:
cd ~/stacks/quantframe-server
cp .env.example .env            # edit QF_PUBLIC_ORIGIN
mkdir -p secrets
openssl rand -hex 32 > secrets/qf_secret_key
printf '%s' 'choose-a-long-password' > secrets/qf_web_password   # at least 12 characters
sudo chown 10001:10001 secrets/* && sudo chmod 600 secrets/*
docker compose up -d --build
docker compose logs -f
```

Open `QF_PUBLIC_ORIGIN` in a browser, sign in with the web password, then sign in to warframe.market.
Only the warframe.market token is kept, encrypted with `qf_secret_key`. Your warframe.market password is never stored.

- **Changing the web password:** edit `secrets/qf_web_password` and restart the container.
- **Losing `qf_secret_key`:** the stored token can't be decrypted, so sign in to warframe.market again.
- **Backups:** the `qf-data` volume holds `quantframe.sqlite`, and a `quantframe.sqlite_backup` copy is made on every start.
