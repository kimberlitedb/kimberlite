---
title: "Deploying on GCP"
section: "operating/cloud"
slug: "gcp"
order: 2
---

# Deploying on GCP

Deploy Kimberlite on Google Cloud Platform.

## Architecture

```
┌───────────────────────────────────────────────────────┐
│                    VPC Network                         │
│  ┌────────────────┐  ┌──────────────┐  ┌────────────┐│
│  │   us-central1-a│  │ us-central1-b│  │us-central1-c││
│  │                │  │              │  │            ││
│  │  ┌──────────┐  │  │  ┌────────┐  │  │ ┌────────┐││
│  │  │  Node 1  │  │  │  │ Node 2 │  │  │ │ Node 3 │││
│  │  │ (Leader) │  │  │  │        │  │  │ │        │││
│  │  └──────────┘  │  │  └────────┘  │  │ └────────┘││
│  └────────────────┘  └──────────────┘  └────────────┘│
└───────────────────────────────────────────────────────┘
```

## Prerequisites

- GCP project with Compute Engine API enabled
- `gcloud` CLI installed and configured
- Terraform or Deployment Manager (optional)

## Machine Type Selection

**Recommended Machine Types:**

| Workload | Machine Type | vCPUs | Memory | Network | Disk |
|----------|--------------|-------|---------|---------|------|
| Small | `e2-standard-2` | 2 | 8 GB | Up to 10 Gbps | 100 GB PD-SSD |
| Medium | `n2-standard-4` | 4 | 16 GB | Up to 10 Gbps | 500 GB PD-SSD |
| Large | `n2-standard-8` | 8 | 32 GB | Up to 16 Gbps | 1 TB PD-SSD |
| Production | `n2-highmem-16` | 16 | 128 GB | 32 Gbps | 2 TB PD-SSD |

**Machine Selection Tips:**
- Use `n2-standard` for balanced workloads
- Use `n2-highcpu` for high-throughput workloads
- Use `n2-highmem` for large projection caches

## Storage Configuration

### Persistent Disk Types

| Disk Type | IOPS (Read/Write) | Throughput | Use Case | Cost |
|-----------|-------------------|------------|----------|------|
| **PD-SSD** | 30/30 per GB (max 100k) | 1,200 MB/s | Recommended | $0.17/GB-month |
| **PD-Balanced** | 6/6 per GB (max 80k) | 240 MB/s | Cost-optimized | $0.10/GB-month |
| **PD-Standard** | 0.75/1.5 per GB | 180 MB/s | NOT for log | $0.04/GB-month |

**Recommendations:**
- **Log disk:** PD-SSD (500 GB = 15k IOPS)
- **Projection disk:** PD-Balanced (200 GB = 1200 IOPS)
- Use separate disks for log and projections

### Disk Configuration

```bash
# Create persistent disks
gcloud compute disks create kimberlite-log-1 \
  --size 500GB \
  --type pd-ssd \
  --zone us-central1-a

gcloud compute disks create kimberlite-proj-1 \
  --size 200GB \
  --type pd-balanced \
  --zone us-central1-a

# Create VM and attach disks
gcloud compute instances create kimberlite-node-1 \
  --machine-type n2-standard-4 \
  --zone us-central1-a \
  --network default \
  --tags kimberlite-node \
  --disk name=kimberlite-log-1,device-name=log,mode=rw,boot=no \
  --disk name=kimberlite-proj-1,device-name=proj,mode=rw,boot=no \
  --service-account kimberlite-sa@project.iam.gserviceaccount.com \
  --scopes cloud-platform

# Format and mount (SSH into VM)
sudo mkfs.ext4 /dev/sdb
sudo mkfs.ext4 /dev/sdc

sudo mkdir -p /var/lib/kimberlite/log
sudo mkdir -p /var/lib/kimberlite/projections

sudo mount /dev/sdb /var/lib/kimberlite/log
sudo mount /dev/sdc /var/lib/kimberlite/projections
```

### /etc/fstab Configuration

```bash
# Add to /etc/fstab
/dev/disk/by-id/google-log  /var/lib/kimberlite/log         ext4  defaults  0 2
/dev/disk/by-id/google-proj /var/lib/kimberlite/projections ext4  defaults  0 2
```

## Networking

### Firewall Rules

```bash
# Create VPC network
gcloud compute networks create kimberlite-vpc \
  --subnet-mode custom

gcloud compute networks subnets create kimberlite-subnet \
  --network kimberlite-vpc \
  --region us-central1 \
  --range 10.0.0.0/24

# Client traffic (from application VPC)
gcloud compute firewall-rules create kimberlite-client \
  --network kimberlite-vpc \
  --allow tcp:7000 \
  --source-ranges 10.1.0.0/24 \
  --target-tags kimberlite-node

# Cluster traffic (between nodes)
gcloud compute firewall-rules create kimberlite-cluster \
  --network kimberlite-vpc \
  --allow tcp:7001 \
  --source-tags kimberlite-node \
  --target-tags kimberlite-node

# Metrics (from monitoring)
gcloud compute firewall-rules create kimberlite-metrics \
  --network kimberlite-vpc \
  --allow tcp:9090 \
  --source-ranges 10.2.0.0/24 \
  --target-tags kimberlite-node

# SSH (from IAP only)
gcloud compute firewall-rules create kimberlite-ssh \
  --network kimberlite-vpc \
  --allow tcp:22 \
  --source-ranges 35.235.240.0/20 \
  --target-tags kimberlite-node
```

### Sole-Tenant Nodes

For consistent performance and compliance:

```bash
# Create sole-tenant node group
gcloud compute sole-tenancy node-groups create kimberlite-group \
  --node-template kimberlite-template \
  --zone us-central1-a \
  --target-size 3

# Create instances on sole-tenant nodes
gcloud compute instances create kimberlite-node-1 \
  --zone us-central1-a \
  --node-group kimberlite-group
```

## Encryption

### At-Rest Encryption with Cloud KMS

```toml
# /etc/kimberlite/config.toml
[encryption]
enabled = true
kms_provider = "gcp-kms"
kms_key_id = "projects/PROJECT_ID/locations/us-central1/keyRings/kimberlite/cryptoKeys/data-key"
```

**Create KMS key:**

```bash
# Create key ring
gcloud kms keyrings create kimberlite \
  --location us-central1

# Create encryption key
gcloud kms keys create data-key \
  --location us-central1 \
  --keyring kimberlite \
  --purpose encryption

# Grant service account access
gcloud kms keys add-iam-policy-binding data-key \
  --location us-central1 \
  --keyring kimberlite \
  --member serviceAccount:kimberlite-sa@project.iam.gserviceaccount.com \
  --role roles/cloudkms.cryptoKeyEncrypterDecrypter
```

### In-Transit Encryption with TLS

Use Google-managed certificates or self-signed:

```bash
# Using Certificate Manager
gcloud certificate-manager certificates create kimberlite-cert \
  --domains=kimberlite.example.com \
  --location=global

# Or use Let's Encrypt
certbot certonly --dns-google -d kimberlite.example.com
```

## IAM Configuration

**Service Account Permissions:**

```bash
# Create service account
gcloud iam service-accounts create kimberlite-sa \
  --display-name "Kimberlite Node Service Account"

# Grant permissions
gcloud projects add-iam-policy-binding PROJECT_ID \
  --member serviceAccount:kimberlite-sa@PROJECT_ID.iam.gserviceaccount.com \
  --role roles/cloudkms.cryptoKeyEncrypterDecrypter

gcloud projects add-iam-policy-binding PROJECT_ID \
  --member serviceAccount:kimberlite-sa@PROJECT_ID.iam.gserviceaccount.com \
  --role roles/compute.viewer

gcloud projects add-iam-policy-binding PROJECT_ID \
  --member serviceAccount:kimberlite-sa@PROJECT_ID.iam.gserviceaccount.com \
  --role roles/monitoring.metricWriter
```

## Deployment with Terraform

```hcl
# main.tf
resource "google_compute_instance" "kimberlite_node" {
  count        = 3
  name         = "kimberlite-node-${count.index + 1}"
  machine_type = "n2-standard-4"
  zone         = element(var.zones, count.index)

  boot_disk {
    initialize_params {
      image = "ubuntu-os-cloud/ubuntu-2204-lts"
      size  = 50
    }
  }

  attached_disk {
    source      = google_compute_disk.log[count.index].self_link
    device_name = "log"
  }

  attached_disk {
    source      = google_compute_disk.proj[count.index].self_link
    device_name = "proj"
  }

  network_interface {
    subnetwork = google_compute_subnetwork.kimberlite.self_link
  }

  service_account {
    email  = google_service_account.kimberlite.email
    scopes = ["cloud-platform"]
  }

  metadata_startup_script = templatefile("startup.sh", {
    node_id = count.index + 1
  })

  tags = ["kimberlite-node"]
}

resource "google_compute_disk" "log" {
  count = 3
  name  = "kimberlite-log-${count.index + 1}"
  type  = "pd-ssd"
  zone  = element(var.zones, count.index)
  size  = 500
}

resource "google_compute_disk" "proj" {
  count = 3
  name  = "kimberlite-proj-${count.index + 1}"
  type  = "pd-balanced"
  zone  = element(var.zones, count.index)
  size  = 200
}
```

## Monitoring Integration

### Cloud Monitoring

Export Kimberlite metrics to Cloud Monitoring:

```bash
# Install ops agent
curl -sSO https://dl.google.com/cloudagents/add-google-cloud-ops-agent-repo.sh
sudo bash add-google-cloud-ops-agent-repo.sh --also-install

# Configure agent
cat > /etc/google-cloud-ops-agent/config.yaml <<'EOF'
metrics:
  receivers:
    prometheus:
      type: prometheus
      config:
        scrape_configs:
          - job_name: kimberlite
            static_configs:
              - targets: ['localhost:9090']
  service:
    pipelines:
      kimberlite:
        receivers: [prometheus]
EOF

sudo systemctl restart google-cloud-ops-agent
```

## Backup Strategy

### Snapshot Schedules

```bash
# Create snapshot schedule
gcloud compute resource-policies create snapshot-schedule kimberlite-daily \
  --region us-central1 \
  --max-retention-days 30 \
  --on-source-disk-delete keep-auto-snapshots \
  --daily-schedule \
  --start-time 03:00

# Attach to disks
gcloud compute disks add-resource-policies kimberlite-log-1 \
  --zone us-central1-a \
  --resource-policies kimberlite-daily
```

## Cost Optimization

**Estimated Monthly Costs (3-node cluster):**

| Component | Configuration | Cost |
|-----------|---------------|------|
| 3x n2-standard-4 | 24/7 | $350 |
| 3x PD-SSD 500 GB | | $255 |
| 3x PD-Balanced 200 GB | | $60 |
| Data transfer | 1 TB/month | $120 |
| **Total** | | **~$785/month** |

**Cost Reduction Tips:**
- Use Committed Use Discounts (up to 57% off)
- Use Sustained Use Discounts (automatic)
- Use Preemptible VMs for non-critical workloads (80% discount)
- Archive old snapshots to Coldline Storage

## Related Documentation

- **[Deployment Guide](../deployment.md)** - General deployment patterns
- **[Configuration Guide](../configuration.md)** - Configuration options
- **[Security Guide](../security.md)** - TLS setup
- **[Monitoring Guide](../monitoring.md)** - Observability

---

**Key Takeaway:** Use PD-SSD for log disk, spread nodes across zones, enable Cloud KMS encryption, and use Cloud Monitoring for observability.
