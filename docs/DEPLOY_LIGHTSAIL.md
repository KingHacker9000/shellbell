# Future Lightsail deployment

Shellbell does not contact, configure, or deploy to Lightsail during development. The planned private URL is `https://shellbell.ashishajin.com`.

A later milestone may provision a small Linux instance, install Docker and Caddy, copy only `deploy/docker-compose.yml`, a private `.env`, and the example Caddy policy, then persist `/data` on backed-up storage. DNS, firewall, TLS verification, secret transfer, OS hardening, backup restoration, and upgrades must be designed and reviewed before that work. Port 8080 should remain private; only 80/443 should reach Caddy. Production must set `SHELLBELL_INSECURE_LOCAL_HTTP=false`.
