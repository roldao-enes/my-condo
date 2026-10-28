# Portable Container Deployment

One OCI image runs the application on a VM, Cloud Run, ECS/Fargate, Kubernetes,
or another container host. PostgreSQL is the only persistent application dependency.
Managed cloud services are optional; the application and its runtime dependencies remain FOSS.

These are deployment templates, not automatic provisioning of cloud accounts, networks,
databases, billing, IAM, or TLS certificates. No cloud resources have been created.

## Runtime Contract

| Setting                                            | Purpose                                                                                                         |
| -------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `PORT`                                             | Listening port; image default `8080`, overridable by the platform                                               |
| `HOST`                                             | Image default `0.0.0.0`                                                                                         |
| `APP_KEY`                                          | Required stable secret, identical across replicas and migration jobs                                            |
| `DB_HOST`, `DB_USER`, `DB_PASSWORD`, `DB_DATABASE` | PostgreSQL connection; socket-directory hosts are also supported                                                |
| `DB_PORT`                                          | Image default `5432`                                                                                            |
| `DB_SSL_MODE`                                      | Image default `verify-full`; `disable` only for local/private test connections or a trusted encrypted connector |
| `DB_SSL_CA`                                        | Optional PEM trust chain from your provider, supplied through its secret manager; escaped newlines are accepted |
| `DB_POOL_MAX`                                      | Maximum connections per process; image default `5`                                                              |
| `TRUST_PROXY`                                      | Optional trusted proxy CIDR or named range supported by proxy-addr; unset trusts no forwarding headers          |

The image includes static assets and production dependencies, runs as UID/GID 1000,
logs to stdout/stderr, and handles SIGTERM through AdonisJS. It contains no `.env` files
or baked-in credentials. No application volume is needed; only `/tmp` and `/app/tmp`
need temporary writable space when using a read-only root filesystem.

`GET /health/live` checks the HTTP process. `GET /health/ready` checks PostgreSQL,
returning 503 on failure. `/health` remains a readiness alias. Use liveness for process
restart decisions and readiness for load-balancer traffic decisions. Cloud Run can use
readiness for its startup probe and liveness for ongoing process checks.

The image never runs migrations automatically on web startup. Run exactly one migration
job per release, wait for exit code 0, then roll out that same image digest. Use
backward-compatible expand/contract migrations while old replicas still serve traffic.
Take a database backup first; changing an image does not reverse database changes.

## Build, Test, and Publish

```sh
docker build -t my-condo:smoke .
docker compose -p condo-smoke -f compose.smoke.yaml up -d --no-build --wait --wait-timeout 120
curl --fail http://127.0.0.1:18080/health/ready
docker compose -p condo-smoke -f compose.smoke.yaml down -v
```

The smoke stack uses only disposable PostgreSQL data and test credentials. It does not
touch the normal Compose stack or local development database.

CI runs the source checks, then tests the built production image with PostgreSQL,
migrations, a non-root user, and a read-only filesystem. It also checks sign-in HTML,
compiled assets, and secret-file exclusion. After success, pushes to `main` and `v*`
release tags publish AMD64/ARM64 images to `ghcr.io/<owner>/<repository>`, including
`sha-<full-commit>` tags, provenance, and an SBOM. ARM64 is built but not runtime-tested
by the AMD64 smoke runner. Enable the repository's Actions/package permissions as needed.

Any OCI registry works; GHCR is just the default publishing destination. For a different registry:

```sh
docker buildx build --platform linux/amd64,linux/arm64 --tag "$IMAGE" --push .
```

Authenticate to that registry first. Use an immutable `@sha256:...` image reference in
production and retain it for rollback; do not deploy mutable `latest` tags. Cloud Run
should use an image copied/published to Artifact Registry; ECS should use ECR or a
registry the task execution role can pull from. Configure image pull credentials on Kubernetes
for private registries. No build arguments or CI variables should contain application secrets.

## VM on Any Provider

Use the existing Compose stack on Compute Engine, EC2, Hetzner, or any Linux VM with
Docker Engine/Compose. Set `.env.production` from `.env.production.example`.
Choose a domain and stable secrets, then either build locally or set `APP_IMAGE` to
your published digest:

```sh
docker compose --env-file .env.production up -d --build
docker compose --env-file .env.production exec app node ace create:admin
```

For a published image instead of a local build:

```sh
docker compose --env-file .env.production pull app migrate
docker compose --env-file .env.production up -d --no-build
```

Use a new digest in `APP_IMAGE` for every release so Compose recreates the migration
task as well as the app. Caddy handles public HTTPS. Only 80/443 are exposed; PostgreSQL
stays private. Compose explicitly disables PostgreSQL TLS on its private Docker network.
Use verified TLS for a remote database. Secure the VM, patch images, restrict ingress,
and automate encrypted off-host backups and restore drills before using real data.

## Google Cloud Run + Cloud SQL

Prerequisites: a project with Cloud Run, Artifact Registry, Cloud SQL Admin, and Secret
Manager APIs enabled; an Artifact Registry repository; a PostgreSQL Cloud SQL instance
and database/user; and a dedicated runtime service account. Grant it Cloud SQL Client,
access to the required secret versions, and registry access where needed. Grant the
deployer permission to use that service account and deploy services/jobs.

Store the stable application key and database password as Secret Manager secrets named
`my-condo-app-key` and `my-condo-db-password`. The examples pin version `1`; update both
the job and service deliberately when rotating. Set `IMAGE` to an Artifact Registry digest,
`REGION`, `INSTANCE` to the Cloud SQL instance connection name (`project:region:instance`),
and `RUNTIME_SERVICE_ACCOUNT` to the service account email.

```sh
gcloud run jobs deploy my-condo-migrate \
  --image "$IMAGE" --region "$REGION" \
  --service-account "$RUNTIME_SERVICE_ACCOUNT" \
  --set-cloudsql-instances "$INSTANCE" \
  --set-env-vars "DB_HOST=/cloudsql/$INSTANCE,DB_USER=condo,DB_DATABASE=my_condo,DB_SSL_MODE=disable" \
  --set-secrets "APP_KEY=my-condo-app-key:1,DB_PASSWORD=my-condo-db-password:1" \
  --command node --args ace,migration:run,--force \
  --tasks 1 --parallelism 1 --max-retries 0 --task-timeout 300s

gcloud run jobs execute my-condo-migrate --region "$REGION" --wait
```

Continue only after migration success:

```sh
gcloud run deploy my-condo \
  --image "$IMAGE" --region "$REGION" \
  --service-account "$RUNTIME_SERVICE_ACCOUNT" \
  --add-cloudsql-instances "$INSTANCE" \
  --set-env-vars "DB_HOST=/cloudsql/$INSTANCE,DB_USER=condo,DB_DATABASE=my_condo,DB_SSL_MODE=disable" \
  --set-secrets "APP_KEY=my-condo-app-key:1,DB_PASSWORD=my-condo-db-password:1" \
  --port 8080 --memory 512Mi --cpu 1 --concurrency 40 \
  --min-instances 0 --max-instances 3 \
  --startup-probe 'httpGet.path=/health/ready,httpGet.port=8080,timeoutSeconds=5,periodSeconds=10,failureThreshold=12' \
  --liveness-probe 'httpGet.path=/health/live,httpGet.port=8080,timeoutSeconds=5,periodSeconds=30,failureThreshold=3' \
  --allow-unauthenticated
```

Here `DB_SSL_MODE=disable` applies only to the local Unix socket; the platform's Cloud SQL
connector encrypts/authenticates the upstream connection. Do not copy that setting to a
direct database endpoint. This example assumes the connector's standard connectivity path;
private-IP-only instances need matching VPC egress/network configuration on both service
and job. `--allow-unauthenticated` exposes the sign-in endpoint, not resident data: application
authentication still applies. Use platform IAM/IAP instead for an entirely restricted endpoint.

Cloud Run does not provide an interactive application shell. For the first administrator,
deploy a second job with the same image, database, and service-account settings, command
`node`, and args `ace,create:admin,--from-env`. Supply `ADMIN_NAME` and `ADMIN_EMAIL` as
environment settings and `ADMIN_PASSWORD` from a separate Secret Manager secret. Execute
it once, check success, then delete the bootstrap job and remove its secret access.
No password belongs in command arguments, source files, images, or log output.

## AWS ECS/Fargate + RDS PostgreSQL

Prerequisites: ECR image repository, ECS cluster, private task subnets with ECR/Secrets
Manager/CloudWatch connectivity (NAT or VPC endpoints), an RDS database/user, an HTTPS ALB
with an IP target group on port 8080, a CloudWatch log group `/ecs/my-condo`, and a task
execution role with image-pull/logging/secret-read permissions (plus KMS decrypt if needed).
Allow task ingress only from the ALB and RDS ingress only from the task security group.

Use Fargate platform 1.4.0+ for JSON-key secret injection. Put `APP_KEY`, `DB_HOST`, `DB_USER`,
`DB_PASSWORD`, `DB_DATABASE`, and `DB_SSL_CA` in a Secrets Manager JSON secret. The CA must be
the current AWS RDS trust bundle; use the real RDS hostname for TLS hostname verification.
Set `IMAGE` to your ECR digest, `AWS_REGION`, `EXECUTION_ROLE_ARN`, and `CONFIG_SECRET_ARN`.
Render the supplied task template using jq rather than editing JSON as text:

```sh
jq --arg image "$IMAGE" --arg region "$AWS_REGION" \
  --arg role "$EXECUTION_ROLE_ARN" --arg secret "$CONFIG_SECRET_ARN" \
  '.executionRoleArn=$role | .containerDefinitions[0].image=$image |
   .containerDefinitions[0].logConfiguration.options["awslogs-region"]=$region |
   .containerDefinitions[0].secrets |= map(.valueFrom=($secret+":"+.name+"::"))' \
  deploy/aws-task-definition.json > task-definition.rendered.json

TASK_DEFINITION=$(aws ecs register-task-definition \
  --cli-input-json file://task-definition.rendered.json \
  --query taskDefinition.taskDefinitionArn --output text)
```

Set `CLUSTER`, `SERVICE`, and `NETWORK_CONFIG` to your existing deployment. For example,
`NETWORK_CONFIG='awsvpcConfiguration={subnets=[subnet-xxx],securityGroups=[sg-xxx],assignPublicIp=DISABLED}'`.
Run migrations as a standalone task using the same revision:

```sh
MIGRATION_TASK=$(aws ecs run-task --cluster "$CLUSTER" --launch-type FARGATE \
  --platform-version LATEST --task-definition "$TASK_DEFINITION" \
  --network-configuration "$NETWORK_CONFIG" \
  --overrides '{"containerOverrides":[{"name":"app","command":["node","ace","migration:run","--force"]}]}' \
  --output json | jq -er '.tasks[0].taskArn')

aws ecs wait tasks-stopped --cluster "$CLUSTER" --tasks "$MIGRATION_TASK"
MIGRATION_EXIT_CODE=$(aws ecs describe-tasks --cluster "$CLUSTER" --tasks "$MIGRATION_TASK" \
  --query 'tasks[0].containers[?name==`app`].exitCode | [0]' --output text)

[[ "$MIGRATION_EXIT_CODE" == '0' ]] && aws ecs update-service \
  --cluster "$CLUSTER" --service "$SERVICE" --task-definition "$TASK_DEFINITION"
```

Create the initial ECS service through your infrastructure tooling/console, using this
task revision, desired count 1, the HTTPS ALB, and the same private network configuration.
Configure the ALB health check as `/health/ready`; the task's own health check uses liveness.
Increase desired count only within the database connection budget. ARM64 Fargate is also
possible by changing `runtimePlatform.cpuArchitecture` to `ARM64` and using the ARM64 image.

For initial administrator setup, run a separate task revision with `ADMIN_NAME`, `ADMIN_EMAIL`,
and a secret-backed `ADMIN_PASSWORD`, overriding its command to
`["node","ace","create:admin","--from-env"]`. Remove bootstrap credentials afterward.
Do not pass the administrator password through `run-task --overrides` plaintext.

## Kubernetes (GKE, EKS, AKS, or Self-Hosted)

Create a namespace-scoped `my-condo` Secret from the populated `.env.cloud` (based on
`.env.cloud.example`), or use your cluster's external-secret integration. Restrict Secret
RBAC and enable encryption at rest. Configure private registry credentials if needed.

```sh
kubectl create secret generic my-condo --from-env-file=.env.cloud
kubectl delete job my-condo-migrate --ignore-not-found
kubectl set image --local -f deploy/kubernetes/migrate.yaml migration="$IMAGE" -o yaml | kubectl apply -f -
kubectl wait --for=condition=complete job/my-condo-migrate --timeout=300s
```

Continue only after migration success:

```sh
kubectl set image --local -f deploy/kubernetes/deployment.yaml app="$IMAGE" -o yaml | kubectl apply -f -
kubectl rollout status deployment/my-condo --timeout=180s
```

On the first deployment, create an internal service:

```sh
kubectl expose deployment my-condo --port=80 --target-port=8080 --type=ClusterIP
kubectl exec -it deployment/my-condo -- node ace create:admin
```

Attach an HTTPS Ingress/Gateway using your cluster's controller, domain, and certificate
mechanism. The manifests deliberately do not assume a cloud-specific load balancer or
certificate manager. Set `TRUST_PROXY` to the actual ingress proxy range, not all addresses,
and restrict direct application access with your cluster's network policy. Templates have
resource limits, separate probes, no service-account token, and a read-only root filesystem.

## Operating Rules

- Keep `APP_KEY` stable across releases/replicas and keep cookie sessions/database-backed
  throttling enabled; no sticky sessions or Redis are needed for this milestone.
- Bound maximum replicas: `DB_POOL_MAX * maximum_instances`, plus jobs and admin connections,
  must fit the database connection budget. Rolling deployments can temporarily double instances.
- Trust forwarding headers only from your real platform/ingress proxies. Without explicit
  trust, IP-based throttling may group clients by the proxy address; never enable blanket trust
  on a publicly reachable application port. Configure this for each chosen platform.
- Separate migration privileges from normal application privileges where feasible.
- Use one release pipeline at a time; do not launch concurrent migration jobs.
- Use provider backups/PITR, encrypted off-host backups, monitoring, restore drills, and
  secret rotation. A container image does not make the unfinished business features production-ready.
- Future email workers must run the same image as separate long-lived workers/jobs, not an
  in-process loop on a request-scaled serverless instance. No email worker exists yet.

The ARM64 Docker image and disposable Compose stack were verified locally on 2026-10-28:
all migrations succeeded, health probes and published-port access passed, compiled JS/CSS
were served, and secret-based administrator creation succeeded. The runtime ran non-root
with a read-only filesystem and excluded environment files and compiled test artifacts.
The smoke stack was removed afterward. CI repeats image smoke checks and must pass
before publishing. Cloud recipes require verification in your target account/network.
