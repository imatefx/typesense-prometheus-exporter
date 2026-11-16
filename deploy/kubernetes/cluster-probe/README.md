# One exporter for a whole cluster

The exporter runs once and Prometheus scrapes `/probe?target=<node>` for each Typesense node, like `blackbox_exporter`. The node list lives in `configmap.yaml` (which nodes and which key the exporter accepts) and in `probe.yaml` (what Prometheus scrapes); keep them the same, written the same way. See [several clusters](../../../docs/configuration.md#several-clusters-with-different-keys).

```bash
kubectl apply -k deploy/kubernetes/cluster-probe
```

The namespace `typesense` appears in `kustomization.yaml`, in the node names in `configmap.yaml` and `probe.yaml` (`typesense-0.typesense.typesense.svc`: pod, headless Service, namespace) and in the prober URL. Change all of them for another namespace.

## Plain Prometheus, without the operator

Skip `probe.yaml` (remove it from `kustomization.yaml`) and add a scrape job, in the same style as the [main README](../../../README.md#quick-start). `typesense-exporter.typesense.svc:8888` is the exporter Service:

```yaml
scrape_configs:
  - job_name: typesense
    metrics_path: /probe
    scrape_interval: 30s
    scrape_timeout: 10s   # above typesense_timeout (5s)
    static_configs:
      - targets:
          - typesense-0.typesense.typesense.svc:8108
          - typesense-1.typesense.typesense.svc:8108
          - typesense-2.typesense.typesense.svc:8108
        labels: {cluster: prod}
    relabel_configs:
      - source_labels: [__address__]
        target_label: __param_target    # the node becomes ?target=<node>
      - source_labels: [__param_target]
        target_label: instance          # instance names the node, not the exporter
      - target_label: __address__
        replacement: typesense-exporter.typesense.svc:8888   # every scrape goes to the exporter
```

Add a second job for the exporter's own health (probe refusals, Typesense request failures, key reload failures). It is a plain scrape of the Service, without `/probe` or relabeling, and the exporter alerts in [`alerts.yml`](../../prometheus/alerts.yml) need it:

```yaml
  - job_name: typesense-exporter-self
    metrics_path: /exporter/metrics
    static_configs:
      - targets: [typesense-exporter.typesense.svc:8888]
```

If Prometheus discovers the nodes through Kubernetes service discovery instead, keep the same three relabel rules and make `__address__` match the `targets` in `configmap.yaml`.

## Using one key for a list of nodes

Instead of `[clusters.prod]`, the config file can hold `probe_allowed_targets` and a default key; see the comment in `configmap.yaml`. A cluster is the better fit when you may add other clusters with other keys later.
