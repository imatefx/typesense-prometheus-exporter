# Kubernetes examples

Plain manifests, no Helm chart. Both run the exporter as user 65534 with a read-only root file system, no capabilities and a `RuntimeDefault` seccomp profile, and probe `/` on port 8888. The image is `imatefx/typesense-prometheus-exporter:v1.2.0`.

| Folder | Use it when | Prometheus resource |
|---|---|---|
| [`single-node/`](single-node) | One exporter next to one Typesense node, scraped at `/metrics` | `ServiceMonitor` |
| [`cluster-probe/`](cluster-probe) | One exporter for a whole cluster, scraped once per node at `/probe` | `Probe` |

Both need the [Prometheus Operator](https://prometheus-operator.dev) CRDs for the `ServiceMonitor` / `Probe`. Without the operator, delete that file from `kustomization.yaml` and use the [plain Prometheus config](cluster-probe/README.md#plain-prometheus-without-the-operator).

## Install

Both folders set `namespace: typesense`, a placeholder for the namespace your Typesense runs in. Change it in `kustomization.yaml` (and in the node and prober names in `cluster-probe/`, see below), then:

```bash
kubectl create namespace typesense   # if it doesn't exist yet
# the key, alone in a file (a trailing newline is fine)
kubectl -n typesense create secret generic typesense-api-key --from-file=api-key=./api-key
kubectl apply -k deploy/kubernetes/single-node
```

The Secret is not part of the kustomization, on purpose: with hot reload, applying a placeholder over the real key would make every scrape fail within seconds. As an alternative to the command above, edit the value in `secret.example.yaml` (in either folder) and `kubectl apply -f` it once; never add it to `resources:`.

**Pick one folder per namespace.** Both create a Deployment and a Service named `typesense-exporter`, so applying both into one namespace makes the second replace the first.

The Secret holds the API key in the key `api-key`; the exporter reads it from `/run/secrets/typesense/api-key`. A [scoped key](../../docs/configuration.md#api-key-permissions) is enough.

If your Prometheus only selects `ServiceMonitor`s and `Probe`s with a label (for example `release: kube-prometheus-stack`), add that label to `servicemonitor.yaml` or `probe.yaml`.

## Pointing at an existing Typesense install

The examples assume Typesense listens on port 8108 behind a Service named `typesense` in the same namespace.

- **single-node:** set `TYPESENSE_HOST` (and `TYPESENSE_PORT`, `TYPESENSE_PROTOCOL`) in `deployment.yaml` to your Service, for example `my-typesense.search.svc`. For a sidecar, add the exporter container to the Typesense pod and use `localhost`. One exporter reports one node: don't point it at a Service that load-balances several nodes.
- **cluster-probe:** list the nodes in `configmap.yaml` (`[clusters.prod] targets`) and, in `probe.yaml` (`static`), written the same way (a target without a port matches an entry with the default port, but keep them identical so it is obvious). For a StatefulSet `typesense` behind a headless Service `typesense` in the namespace `typesense`, the nodes are `typesense-0.typesense.typesense.svc:8108`, `typesense-1....`. Use `protocol = "https"` if Typesense serves TLS.

The `Probe`'s `prober.url` must be a name Prometheus can resolve from its own namespace: `typesense-exporter.<namespace>.svc:8888`.

## Rotating the API key

The key is mounted from the Secret as a file, and the exporter re-reads a key file when it changes (since 1.2.0), so no restart is needed:

1. Add the new key to Typesense.
2. Update the Secret, for example `kubectl -n typesense create secret generic typesense-api-key --from-file=api-key=./api-key --dry-run=client -o yaml | kubectl apply -f -`.
3. Wait for the kubelet to sync the volume, which typically takes 1 to 2 minutes. The exporter then logs `reloaded the Typesense API key from /run/secrets/typesense/api-key` on its first scrape after that.
4. Remove the old key from Typesense.

If the key is wrong, `typesense_health` stays 1, because `/health` needs no key. Check `typesense_scrape_success{endpoint!="/health"} == 0`, which the bundled `TypesenseScrapeFailing` alert in [`deploy/prometheus/alerts.yml`](../prometheus/alerts.yml) fires on.

The Secret must be mounted as a directory (as in these manifests), not with `subPath`: Kubernetes never updates `subPath` mounts. See [rotating keys](../../docs/configuration.md#rotating-keys).

## Validating changes

`scripts/validate-kubernetes.sh` renders both folders with kustomize and checks them (and the `secret.example.yaml` files) with kubeconform, including the `ServiceMonitor` and `Probe` schemas. CI runs it. It needs Docker.
