# Deploying on Azure

Deploy Kimberlite on Microsoft Azure.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  Virtual Network (VNet)                  │
│  ┌──────────────┐  ┌────────────────┐  ┌──────────────┐│
│  │ Zone 1       │  │   Zone 2       │  │   Zone 3     ││
│  │              │  │                │  │              ││
│  │  ┌────────┐  │  │  ┌──────────┐  │  │  ┌────────┐ ││
│  │  │ Node 1 │  │  │  │  Node 2  │  │  │  │ Node 3 │ ││
│  │  │(Leader)│  │  │  │          │  │  │  │        │ ││
│  │  └────────┘  │  │  └──────────┘  │  │  └────────┘ ││
│  └──────────────┘  └────────────────┘  └──────────────┘│
└─────────────────────────────────────────────────────────┘
```

## Prerequisites

- Azure subscription with Virtual Machines permissions
- Azure CLI installed and authenticated
- Terraform or ARM templates (optional)

## VM Size Selection

**Recommended VM Sizes:**

| Workload | VM Size | vCPUs | Memory | Network | Disk |
|----------|---------|-------|---------|---------|------|
| Small | `Standard_D2s_v5` | 2 | 8 GB | 12.5 Gbps | 100 GB Premium SSD |
| Medium | `Standard_D4s_v5` | 4 | 16 GB | 12.5 Gbps | 500 GB Premium SSD |
| Large | `Standard_D8s_v5` | 8 | 32 GB | 12.5 Gbps | 1 TB Premium SSD |
| Production | `Standard_D16s_v5` | 16 | 64 GB | 12.5 Gbps | 2 TB Premium SSD |

**VM Selection Tips:**
- Use `Dsv5` series for balanced workloads
- Use `Fsv2` series for compute-intensive workloads
- Use `Esv5` series for memory-intensive workloads
- Use availability zones for high availability

## Storage Configuration

### Managed Disk Types

| Disk Type | IOPS | Throughput | Use Case | Cost |
|-----------|------|------------|----------|------|
| **Premium SSD** | 120-20,000 | 25-900 MB/s | Recommended for log | $0.135/GB-month |
| **Standard SSD** | 500-6,000 | 60-750 MB/s | Cost-optimized | $0.075/GB-month |
| **Ultra Disk** | 300-160,000 | 2,000 MB/s | Ultra-high performance | $0.12/GB-month + IOPS cost |
| **Standard HDD** | 500-2,000 | 60-500 MB/s | NOT for log | $0.045/GB-month |

**Recommendations:**
- **Log disk:** Premium SSD P30 (1 TB, 5000 IOPS)
- **Projection disk:** Standard SSD E20 (512 GB, 500 IOPS)
- Use separate disks for log and projections

### Disk Configuration

```bash
# Create resource group
az group create \
  --name kimberlite-rg \
  --location eastus

# Create managed disks
az disk create \
  --resource-group kimberlite-rg \
  --name kimberlite-log-1 \
  --size-gb 1024 \
  --sku Premium_LRS \
  --zone 1

az disk create \
  --resource-group kimberlite-rg \
  --name kimberlite-proj-1 \
  --size-gb 512 \
  --sku StandardSSD_LRS \
  --zone 1

# Create VM with attached disks
az vm create \
  --resource-group kimberlite-rg \
  --name kimberlite-node-1 \
  --image Ubuntu2204 \
  --size Standard_D4s_v5 \
  --zone 1 \
  --vnet-name kimberlite-vnet \
  --subnet default \
  --assign-identity \
  --attach-data-disks kimberlite-log-1 kimberlite-proj-1

# Format and mount (SSH into VM)
sudo mkfs.ext4 /dev/sdc
sudo mkfs.ext4 /dev/sdd

sudo mkdir -p /var/lib/kimberlite/log
sudo mkdir -p /var/lib/kimberlite/projections

sudo mount /dev/sdc /var/lib/kimberlite/log
sudo mount /dev/sdd /var/lib/kimberlite/projections
```

### /etc/fstab Configuration

```bash
# Find disk UUIDs
sudo blkid

# Add to /etc/fstab
UUID=xxx  /var/lib/kimberlite/log          ext4  defaults  0 2
UUID=yyy  /var/lib/kimberlite/projections  ext4  defaults  0 2
```

## Networking

### Network Security Groups

```bash
# Create VNet
az network vnet create \
  --resource-group kimberlite-rg \
  --name kimberlite-vnet \
  --address-prefix 10.0.0.0/16 \
  --subnet-name default \
  --subnet-prefix 10.0.0.0/24

# Create NSG
az network nsg create \
  --resource-group kimberlite-rg \
  --name kimberlite-nsg

# Client traffic (from application subnet)
az network nsg rule create \
  --resource-group kimberlite-rg \
  --nsg-name kimberlite-nsg \
  --name allow-client \
  --priority 100 \
  --source-address-prefixes 10.1.0.0/24 \
  --destination-port-ranges 7000 \
  --protocol Tcp \
  --access Allow

# Cluster traffic (between nodes)
az network nsg rule create \
  --resource-group kimberlite-rg \
  --nsg-name kimberlite-nsg \
  --name allow-cluster \
  --priority 110 \
  --source-address-prefixes VirtualNetwork \
  --destination-port-ranges 7001 \
  --protocol Tcp \
  --access Allow

# Metrics (from monitoring subnet)
az network nsg rule create \
  --resource-group kimberlite-rg \
  --nsg-name kimberlite-nsg \
  --name allow-metrics \
  --priority 120 \
  --source-address-prefixes 10.2.0.0/24 \
  --destination-port-ranges 9090 \
  --protocol Tcp \
  --access Allow

# SSH (from bastion only)
az network nsg rule create \
  --resource-group kimberlite-rg \
  --nsg-name kimberlite-nsg \
  --name allow-ssh \
  --priority 130 \
  --source-address-prefixes AzureBastionSubnet \
  --destination-port-ranges 22 \
  --protocol Tcp \
  --access Allow
```

### Proximity Placement Groups

For lowest latency between nodes:

```bash
az ppg create \
  --resource-group kimberlite-rg \
  --name kimberlite-ppg \
  --type Standard
```

## Encryption

### At-Rest Encryption with Azure Key Vault

```toml
# /etc/kimberlite/config.toml
[encryption]
enabled = true
kms_provider = "azure-keyvault"
kms_key_id = "https://kimberlite-kv.vault.azure.net/keys/data-key/version"
```

**Create Key Vault and key:**

```bash
# Create Key Vault
az keyvault create \
  --resource-group kimberlite-rg \
  --name kimberlite-kv \
  --location eastus \
  --enable-rbac-authorization

# Create encryption key
az keyvault key create \
  --vault-name kimberlite-kv \
  --name data-key \
  --kty RSA \
  --size 2048

# Grant VM managed identity access
VM_IDENTITY=$(az vm identity show \
  --resource-group kimberlite-rg \
  --name kimberlite-node-1 \
  --query principalId -o tsv)

az role assignment create \
  --assignee $VM_IDENTITY \
  --role "Key Vault Crypto User" \
  --scope /subscriptions/SUBSCRIPTION_ID/resourceGroups/kimberlite-rg/providers/Microsoft.KeyVault/vaults/kimberlite-kv
```

### In-Transit Encryption with TLS

Use Azure certificates or self-signed:

```bash
# Using App Service Certificate
az appservice certificate create \
  --resource-group kimberlite-rg \
  --name kimberlite-cert \
  --hostname kimberlite.example.com

# Or use Let's Encrypt
certbot certonly --dns-azure -d kimberlite.example.com
```

## Identity and Access Management

**Managed Identity Configuration:**

```bash
# Enable system-assigned identity
az vm identity assign \
  --resource-group kimberlite-rg \
  --name kimberlite-node-1

# Grant permissions
az role assignment create \
  --assignee $VM_IDENTITY \
  --role "Key Vault Crypto User" \
  --scope /subscriptions/SUBSCRIPTION_ID/resourceGroups/kimberlite-rg

az role assignment create \
  --assignee $VM_IDENTITY \
  --role "Monitoring Metrics Publisher" \
  --scope /subscriptions/SUBSCRIPTION_ID/resourceGroups/kimberlite-rg
```

## Deployment with Terraform

```hcl
# main.tf
resource "azurerm_linux_virtual_machine" "kimberlite_node" {
  count               = 3
  name                = "kimberlite-node-${count.index + 1}"
  resource_group_name = azurerm_resource_group.main.name
  location            = azurerm_resource_group.main.location
  size                = "Standard_D4s_v5"
  zone                = count.index + 1

  network_interface_ids = [
    azurerm_network_interface.main[count.index].id,
  ]

  os_disk {
    caching              = "ReadWrite"
    storage_account_type = "Premium_LRS"
  }

  source_image_reference {
    publisher = "Canonical"
    offer     = "0001-com-ubuntu-server-jammy"
    sku       = "22_04-lts-gen2"
    version   = "latest"
  }

  identity {
    type = "SystemAssigned"
  }

  custom_data = base64encode(templatefile("cloud-init.yaml", {
    node_id = count.index + 1
  }))
}

resource "azurerm_managed_disk" "log" {
  count                = 3
  name                 = "kimberlite-log-${count.index + 1}"
  location             = azurerm_resource_group.main.location
  resource_group_name  = azurerm_resource_group.main.name
  storage_account_type = "Premium_LRS"
  create_option        = "Empty"
  disk_size_gb         = 1024
  zone                 = count.index + 1
}

resource "azurerm_virtual_machine_data_disk_attachment" "log" {
  count              = 3
  managed_disk_id    = azurerm_managed_disk.log[count.index].id
  virtual_machine_id = azurerm_linux_virtual_machine.kimberlite_node[count.index].id
  lun                = 0
  caching            = "None"
}
```

## Monitoring Integration

### Azure Monitor

Export Kimberlite metrics to Azure Monitor:

```bash
# Install Azure Monitor agent
wget https://aka.ms/downloadamagent -O InstallAzureMonitorAgent.sh
sudo bash InstallAzureMonitorAgent.sh

# Configure data collection rule
az monitor data-collection-rule create \
  --name kimberlite-dcr \
  --resource-group kimberlite-rg \
  --location eastus \
  --rule-file dcr.json
```

**dcr.json:**

```json
{
  "dataSources": {
    "prometheusForwarder": [{
      "streams": ["Microsoft-PrometheusMetrics"],
      "name": "kimberlite-metrics"
    }]
  },
  "destinations": {
    "azureMonitorMetrics": {
      "name": "default"
    }
  },
  "dataFlows": [{
    "streams": ["Microsoft-PrometheusMetrics"],
    "destinations": ["default"]
  }]
}
```

## Backup Strategy

### Azure Backup

```bash
# Create Recovery Services vault
az backup vault create \
  --resource-group kimberlite-rg \
  --name kimberlite-vault \
  --location eastus

# Enable backup for VMs
az backup protection enable-for-vm \
  --resource-group kimberlite-rg \
  --vault-name kimberlite-vault \
  --vm kimberlite-node-1 \
  --policy-name DefaultPolicy
```

### Disk Snapshots

```bash
# Create snapshot
az snapshot create \
  --resource-group kimberlite-rg \
  --name kimberlite-log-snapshot \
  --source kimberlite-log-1

# Schedule snapshots (use Logic Apps or Automation)
```

## Cost Optimization

**Estimated Monthly Costs (3-node cluster):**

| Component | Configuration | Cost |
|-----------|---------------|------|
| 3x Standard_D4s_v5 | 24/7 | $380 |
| 3x Premium SSD 1 TB | P30 | $420 |
| 3x Standard SSD 512 GB | E20 | $75 |
| Data transfer | 1 TB/month | $87 |
| **Total** | | **~$962/month** |

**Cost Reduction Tips:**
- Use Azure Reserved Instances (up to 72% discount)
- Use Spot VMs for non-critical workloads (up to 90% discount)
- Use Standard SSD instead of Premium SSD where possible
- Delete old snapshots regularly

## Related Documentation

- **[Deployment Guide](../deployment.md)** - General deployment patterns
- **[Configuration Guide](../configuration.md)** - Configuration options
- **[Security Guide](../security.md)** - TLS setup
- **[Monitoring Guide](../monitoring.md)** - Observability

---

**Key Takeaway:** Use Premium SSD for log disk, deploy across availability zones, enable Key Vault encryption, and use Azure Monitor for observability.
