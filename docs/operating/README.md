---
title: "Operating - Running in Production"
section: "operating"
slug: "README"
order: 0
---

# Operating - Running in Production

Deploy, configure, and maintain Kimberlite clusters.

## Deployment

**[Deployment Guide](deployment.md)** - How to deploy Kimberlite

Deploy single-node, multi-node clusters, and multi-region configurations:
- Single-node deployment for development/testing
- 3-node cluster for fault tolerance
- Multi-region for data sovereignty
- Docker and Kubernetes deployments

## Configuration

**[Configuration Guide](configuration.md)** - Configuration options

Configure Kimberlite for your environment:
- TOML configuration file format
- Environment variables and CLI flags
- Storage, consensus, and security settings
- Production configuration examples

## Security

**[Security Guide](security.md)** - TLS and authentication

Secure your Kimberlite deployment:
- TLS configuration (mutual TLS recommended)
- Authentication and authorization
- Encryption at rest (per-tenant envelope encryption)
- Compliance certifications (HIPAA, SOC 2, GDPR)

## Monitoring

**[Monitoring Guide](monitoring.md)** - Observability

Monitor cluster health and performance:
- Prometheus metrics (latency, throughput, health)
- Structured logging (JSON format)
- Distributed tracing (OpenTelemetry)
- Alerting rules and dashboards

## Performance

**[Performance Guide](performance.md)** - Tuning

Optimize for your workload:
- Throughput tuning (batch sizes, parallelism)
- Latency optimization (fsync modes, disk selection)
- Memory management (projection caching)
- Benchmark results and profiling

## Troubleshooting

**[Troubleshooting Guide](troubleshooting.md)** - Debug issues

Diagnose and fix common problems:
- Cluster has no leader
- High write latency
- Projection lag growing
- Frequent view changes
- Data corruption recovery

## Cloud Deployments

**Cloud-specific deployment guides:**

- **[AWS](cloud/aws.md)** - Deploy on Amazon Web Services
- **[GCP](cloud/gcp.md)** - Deploy on Google Cloud Platform
- **[Azure](cloud/azure.md)** - Deploy on Microsoft Azure

## Production Checklist

Before going to production, verify:

### Security
- [ ] TLS enabled with valid certificates
- [ ] Client certificate authentication configured
- [ ] At-rest encryption enabled with KMS
- [ ] Network security groups/firewall rules configured

### High Availability
- [ ] At least 3 nodes deployed across availability zones
- [ ] Cluster can survive 1 node failure
- [ ] Backups configured with retention policy

### Monitoring
- [ ] Prometheus scraping metrics
- [ ] Alerts configured for critical issues
- [ ] Logs aggregated to centralized system
- [ ] Dashboards created for key metrics

### Performance
- [ ] Load testing completed
- [ ] Disk I/O meets requirements (>3000 IOPS for log)
- [ ] Network latency between nodes <5ms
- [ ] SLAs defined and validated

---

**Key Takeaway:** Start with the deployment guide, secure with TLS and encryption, monitor with Prometheus, and debug with the troubleshooting guide.
