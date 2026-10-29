# UpCloud Onboarding Checklist

This runbook deploys the application to one UpCloud VM with a separate UpCloud
Managed PostgreSQL database. A failed app VM does not take down the database, but
the website remains unavailable until the VM is recovered. This is not a
high-availability deployment.

The planning estimate was EUR 6/month for a 1-core, 2 GB RAM, 20 GB VM plus
EUR 9/month for the entry managed database: approximately EUR 180/year before tax.
Confirm current prices, region availability, backup retention, and any extra
networking charges in the control panel before creating resources.

Use the standalone [cloud Compose file](../compose.cloud.yaml), not the
[local-database stack](../compose.yaml). Commands below run from the deployment
directory on the VM unless stated otherwise. Stop at any failed check.

## 1. Prepare the Release and Deployment Configuration

- [ ] Merge the application and workflow changes, and ensure CI passes.
- [ ] Publish a release using a version tag such as `v1.0.0`.
- [ ] Record its image digest from GHCR. Deploy
      `ghcr.io/roldao-enes/my-condo@sha256:<actual-digest>`, not `latest`.
- [ ] Copy `compose.cloud.yaml`, `Caddyfile`, and `.env.cloud.example` from that
      release to a stable directory on the VM. Do not mix deployment files from
      unrelated revisions.

The workflow publishes images and GitHub Releases; it does not provision UpCloud
resources or deploy to the VM.

## 2. Provision the VM and Managed PostgreSQL

UpCloud control-panel labels and networking options can vary. Use its current
connection instructions as the source of truth for endpoint names, ports, network
support, and CA certificates. Do not assume the managed service listens on 5432.

### 2.1 Create the Application VM

1. Sign in to the UpCloud control panel and choose a region close to your users.
   For resident data, select a location appropriate to your data-protection
   obligations.
2. Select the planned Starter VM: 1 CPU core, 2 GB RAM, and 20 GB storage.
3. Use a supported Ubuntu LTS image and your SSH public key. Keep the private key
   on your own machine, not in Git or the container image.
4. Record the VM's public IPv4 address. If using private database connectivity,
   attach the VM to the appropriate private network and record its private IP.
5. Restrict SSH to trusted source addresses. Allow inbound TCP 80/443 for Caddy;
   UDP 443 is optional for HTTP/3. Do not publish application port 8080 or open a
   database port inbound on the app VM.

### 2.2 Create the Managed PostgreSQL Service

1. In UpCloud's managed database area, choose **PostgreSQL**, not a database
   installed on the app VM.
2. Prefer the same region as the VM to reduce latency and simplify connectivity.
3. Select the entry plan if it still meets the budget and connection/storage
   requirements. Use PostgreSQL 17 when available; the application supports
   PostgreSQL 15+ and CI currently uses 17.
4. Review the selected plan's actual backup retention, point-in-time recovery,
   maintenance window, and availability/failover guarantees. A managed database
   is separate from the VM, but an entry single-node service is not necessarily
   highly available.
5. Wait until the service is ready. Save its generated administrator credentials
   in a password manager.
6. Record the service-provided hostname, port, available database names, and TLS
   instructions. Download the CA certificate from an authenticated UpCloud source
   if required.

Do not copy the full connection URI into logs, tickets, or source code: it can
contain a password. The app uses separate `DB_*` fields, so no URI password
percent-encoding is needed.

### 2.3 Allow Only the Application VM to Connect

**Preferred: private connectivity**

1. If supported by the selected service, configure its private network connection
   according to UpCloud's instructions and ensure the VM can route to that network.
2. Choose the provider's private endpoint hostname. Confirm it resolves on the VM
   and is covered by the database certificate.
3. If the service has an IP allowlist, allow only the VM's actual private source
   address or the smallest required network range.
4. Disable public database access if the service supports doing so and no other
   approved client requires it.

**Alternative: restricted public endpoint**

1. Use the service's public hostname and enable TLS verification.
2. In the database's access controls, allow only the app VM's public egress IPv4
   address as a `/32` rule. If outbound traffic uses NAT, allow the actual NAT
   egress address instead.
3. Never allow `0.0.0.0/0` or `::/0`. If IPv6 is used, explicitly restrict the
   applicable IPv6 source address too.
4. If you temporarily allow your workstation to bootstrap the database, remove
   that access when finished.

For either option, permit outbound traffic from the VM/container network to the
database hostname and its actual port. DNS must also work. Docker normally
masquerades container traffic through the VM; do not allowlist a Docker bridge
address such as `172.18.0.2`. Verify the source address actually used on your route.

### 2.4 Create the Application Login and Database

Use UpCloud's user/database controls where available. Otherwise, connect using a
PostgreSQL client and the provider's generated administrator credentials, using
`sslmode=verify-full` and its CA when required. Let the client prompt for the
password; do not put it in a shell command or connection URI.

Create:

- A dedicated login named `condo`, with a strong unique password.
- A database named `my_condo`, owned by `condo`.

For a service whose administrator has the required privileges, the equivalent
commands in an interactive `psql` session are:

```sql
CREATE ROLE condo LOGIN;
\password condo
CREATE DATABASE my_condo OWNER condo;
```

`\password` prompts without putting the plaintext password in the SQL command.
These are one-time commands for new resources; do not rerun them blindly.
If the managed administrator cannot create roles or databases this way, use the
provider-supported controls rather than attempting to obtain superuser access.

The current cloud stack uses one login for both migrations and normal requests.
It needs permission to create and alter application tables in the database, but
should not be a cluster superuser. Owning the dedicated database is a simple
starting configuration. Separating migration and runtime roles is an optional
later hardening step that requires additional configuration.

### 2.5 Configure the Application Connection

On the VM, prepare the ignored production environment file:

```sh
cp .env.cloud.example .env.cloud
chmod 600 .env.cloud
```

Edit it with a trusted editor:

```dotenv
APP_DOMAIN=condo.example.org
APP_IMAGE=ghcr.io/roldao-enes/my-condo@sha256:<actual-release-digest>
APP_KEY=<unique-stable-application-key>
DB_HOST=<provider-endpoint-hostname>
DB_PORT=<provider-port>
DB_USER=condo
DB_PASSWORD='<actual-database-password>'
DB_DATABASE=my_condo
DB_SSL_MODE=verify-full
DB_SSL_CA=
DB_POOL_MAX=5
```

Replace every placeholder. Generate `APP_KEY` with
`node ace generate:key --show` in a trusted development checkout, then keep a secure
copy outside the VM. Do not regenerate it on each deployment.

Compose environment files support quoting: single-quoted values avoid `$`
interpolation in passwords. Follow Compose's escaping rules if a password itself
contains a single quote. Do not `source .env.cloud` as a shell script.

The cloud Compose file always sets `DB_SSL_MODE=verify-full`; changing that value
in the environment file does not disable verification.

If UpCloud supplies a private CA, set `DB_SSL_CA` to the **PEM contents**, not a
file path, using literal `\n` sequences between lines:

```dotenv
DB_SSL_CA='-----BEGIN CERTIFICATE-----\n<actual-certificate-body>\n-----END CERTIFICATE-----'
```

Leave it empty only when the database certificate chain is trusted by the image's
default trust store. Keep `DB_HOST` as the certificate's hostname rather than
replacing it with an IP address. Never fix certificate errors by disabling TLS.

`DB_POOL_MAX=5` bounds connections per app or migration process. During an upgrade,
the existing app and migration job can each open connections; leave capacity for
both and for administration within the managed database's connection limit.

### 2.6 Verify the Connection from the Application Container

After installing Docker as described in step 3, authenticate to GHCR if the package
is private, using a token with read-only package access. Validate without printing
resolved secrets and pull the images:

```sh
docker compose -f compose.cloud.yaml --env-file .env.cloud config --quiet &&
docker compose -f compose.cloud.yaml --env-file .env.cloud --profile tools pull
```

Before creating tables, run this read-only connection check using the exact app
image, Docker network, credentials, and TLS settings:

```sh
docker compose -f compose.cloud.yaml --env-file .env.cloud run --rm -T migrate \
  node --input-type=module <<'JS'
import assert from 'node:assert/strict'
import pg from 'pg'

assert.equal(process.env.DB_SSL_MODE, 'verify-full')
const client = new pg.Client({
  host: process.env.DB_HOST,
  port: Number(process.env.DB_PORT),
  user: process.env.DB_USER,
  password: process.env.DB_PASSWORD,
  database: process.env.DB_DATABASE,
  connectionTimeoutMillis: 10000,
  ssl: {
    rejectUnauthorized: true,
    ...(process.env.DB_SSL_CA
      ? { ca: process.env.DB_SSL_CA.replace(/\\n/g, '\n') }
      : {}),
  },
})

try {
  await client.connect()
  const { rows } = await client.query(
    'SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()'
  )
  assert.equal(rows[0]?.ssl, true, 'Database connection must use TLS')
  console.log('Managed PostgreSQL connection and verified TLS succeeded')
} finally {
  await client.end()
}
JS
```

Success confirms database login and verified TLS from the container, without
running migrations. Failure exits nonzero; resolve it before proceeding:

| Symptom                           | Check                                                                                |
| --------------------------------- | ------------------------------------------------------------------------------------ |
| Hostname lookup failure           | Provider hostname, VM DNS, private network DNS availability                          |
| Connection timeout/refused        | Actual service port, service readiness, routing, outbound rules, source IP allowlist |
| Certificate validation failure    | Correct hostname, provider CA contents, VM clock; do not disable verification        |
| Authentication failure            | Login/password, quoting in `.env.cloud`, current credentials                         |
| Database does not exist           | Create `my_condo` or correct `DB_DATABASE`                                           |
| Migration permission denied later | Application database ownership and schema privileges                                 |

If moving existing data, back it up and restore it into the managed database using
the provider-supported procedure before exposing the new app. Verify that data and
schema before applying any pending migrations. Keep the original backup off-VM.

## 3. Prepare the VM, Docker, and Domain

- [ ] Install Docker Engine and its Compose plugin using the
      [official Ubuntu instructions](https://docs.docker.com/engine/install/ubuntu/).
- [ ] Configure OS security updates and verify the host firewall and UpCloud
      firewall rules. Docker-published ports need particular attention; only
      Caddy should be public.
- [ ] Point the domain's DNS A record to the VM. Add an AAAA record only if IPv6
      is configured and reachable.
- [ ] Confirm `docker compose version` works and Docker starts on boot.
- [ ] Complete the container-to-database connection check in step 2.6.

## 4. Deploy and Verify

Take a database backup/recovery point before upgrades. Run one deployment at a
time, and stop if migrations fail:

```sh
docker compose -f compose.cloud.yaml --env-file .env.cloud run --rm migrate &&
docker compose -f compose.cloud.yaml --env-file .env.cloud up -d --wait --wait-timeout 180
```

On first deployment only, create the administrator interactively:

```sh
docker compose -f compose.cloud.yaml --env-file .env.cloud exec app node ace create:admin
```

- [ ] Check `https://<your-domain>/health/ready` returns HTTP 200.
- [ ] Verify a valid HTTPS certificate, administrator login, and representative
      read/write flows. Do not seed demo data in production.
- [ ] Confirm that only ports 80/443 and restricted SSH are reachable externally.

## 5. Establish the Release Procedure

- [ ] For each release, record the old image digest, update `APP_IMAGE`, pull,
      run migrations once, then start and verify the app as above.
- [ ] Retain the previous image in GHCR. Application rollback uses its digest;
      it does not reverse database migrations.
- [ ] Use backward-compatible migrations: the old app may still run while a
      migration executes.
- [ ] If automating later, use a protected GitHub `production` environment and
      serialize deployment jobs. Do not store production secrets in the image.

## 6. Rehearse Recovery and Monitor

- [ ] Verify the actual managed backup/PITR retention and test restoration into a
      separate database. A backup setting alone is not proof of recoverability.
- [ ] Keep secure off-VM copies of `APP_KEY`, connection settings, deployment
      configuration, and the known-good release digest.
- [ ] Rehearse replacing the VM, reconnecting it to the database, updating its
      access rules/DNS as needed, and verifying the app.
- [ ] Monitor uptime, VM disk/memory, database storage, and connection usage.
- [ ] Define audit-log retention: cloud Compose rotates stdout logs, while audit
      files in `/app/tmp` are ephemeral. Forward logs off-VM if they must survive a
      VM failure.
- [ ] Consider independent encrypted database dumps for longer retention or
      provider/account-level recovery; price their storage separately.

## References

- [UpCloud pricing](https://upcloud.com/pricing/)
- [UpCloud documentation](https://upcloud.com/docs/)
- [Portable deployment guide](deployment.md#upcloud-vm-with-managed-postgresql)
- [Cloud environment template](../.env.cloud.example)
- [Compose environment-file syntax](https://docs.docker.com/compose/how-tos/environment-variables/variable-interpolation/#env-file-syntax)
