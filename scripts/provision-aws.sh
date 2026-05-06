#!/usr/bin/env bash
# Provision AWS EC2 instances for Ra vs PostgreSQL production benchmarking.
#
# Creates the infrastructure described in the project plan:
#   - Primary benchmark instance (c5n.metal, Intel, us-east-2)
#   - ARM comparison instance (c6g.metal, Graviton2, us-east-2)
#
# Prerequisites:
#   aws cli v2 installed and configured (aws configure)
#   Key pair already created in the target region
#
# Usage:
#   AWS_PROFILE=myprofile KEY_NAME=my-keypair ./scripts/provision-aws.sh
#
# Environment variables:
#   AWS_PROFILE     AWS CLI profile (default: default)
#   KEY_NAME        EC2 key pair name (required)
#   REGION          AWS region (default: us-east-2)
#   INSTANCE_INTEL  Intel instance type (default: c5n.metal)
#   INSTANCE_ARM    ARM instance type (default: c6g.metal)
#   DRY_RUN         Set to 1 to print commands without executing
#   SPOT            Set to 1 to use Spot instances (~70% savings)
#   TAG_PREFIX      Resource tag prefix (default: ra-bench)

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
AWS_PROFILE="${AWS_PROFILE:-default}"
KEY_NAME="${KEY_NAME:-}"
REGION="${REGION:-us-east-2}"
INSTANCE_INTEL="${INSTANCE_INTEL:-c5n.metal}"
INSTANCE_ARM="${INSTANCE_ARM:-c6g.metal}"
DRY_RUN="${DRY_RUN:-0}"
SPOT="${SPOT:-0}"
TAG_PREFIX="${TAG_PREFIX:-ra-bench}"

AWS="aws --profile ${AWS_PROFILE} --region ${REGION}"
[[ "${DRY_RUN}" == "1" ]] && AWS="echo [DRY] aws --profile ${AWS_PROFILE} --region ${REGION}"

log() { echo "[provision-aws] $*"; }

if [[ -z "${KEY_NAME}" ]]; then
    echo "ERROR: KEY_NAME must be set (name of your EC2 key pair)"
    echo "  Create one: aws ec2 create-key-pair --key-name ra-bench-key | jq -r .KeyMaterial > ra-bench-key.pem"
    exit 1
fi

# ---------------------------------------------------------------------------
# Step 1: Find the latest Amazon Linux 2023 AMI
# ---------------------------------------------------------------------------
log "Step 1: Resolving AMI for ${REGION}"

AMI_INTEL=$(${AWS} ssm get-parameter \
    --name "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64" \
    --query "Parameter.Value" \
    --output text)

AMI_ARM=$(${AWS} ssm get-parameter \
    --name "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-arm64" \
    --query "Parameter.Value" \
    --output text)

log "  Intel AMI : ${AMI_INTEL}"
log "  ARM AMI   : ${AMI_ARM}"

# ---------------------------------------------------------------------------
# Step 2: Create Security Group
# ---------------------------------------------------------------------------
log "Step 2: Creating security group"

SG_NAME="${TAG_PREFIX}-sg-$(date +%Y%m%d)"

SG_ID=$(${AWS} ec2 create-security-group \
    --group-name "${SG_NAME}" \
    --description "Ra benchmark security group" \
    --query "GroupId" \
    --output text)

log "  Security group: ${SG_ID}"

# Allow SSH from anywhere (restrict this in production)
${AWS} ec2 authorize-security-group-ingress \
    --group-id "${SG_ID}" \
    --protocol tcp \
    --port 22 \
    --cidr 0.0.0.0/0 > /dev/null

# Allow inter-instance traffic (for distributed simulation)
${AWS} ec2 authorize-security-group-ingress \
    --group-id "${SG_ID}" \
    --protocol all \
    --source-group "${SG_ID}" > /dev/null

# ---------------------------------------------------------------------------
# Step 3: Create placement group for low-latency inter-instance networking
# ---------------------------------------------------------------------------
log "Step 3: Creating placement group"

PG_NAME="${TAG_PREFIX}-pg-$(date +%Y%m%d)"
${AWS} ec2 create-placement-group \
    --group-name "${PG_NAME}" \
    --strategy cluster > /dev/null

# ---------------------------------------------------------------------------
# Step 4: Generate user-data bootstrap script
# ---------------------------------------------------------------------------
log "Step 4: Generating bootstrap script"

USERDATA=$(base64 -w0 <<'BOOTSTRAP'
#!/bin/bash
set -euo pipefail
log() { echo "[bootstrap] $*" | tee -a /var/log/ra-bootstrap.log; }

log "Installing system dependencies"
dnf update -y
dnf install -y \
    git gcc make cmake \
    readline-devel zlib-devel openssl-devel libxml2-devel \
    flex bison python3 python3-pip \
    mdadm xfsprogs nvme-cli \
    tmux htop iotop sysstat

log "Installing Rust"
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
source /root/.cargo/env

log "Cloning Ra repository"
git clone https://github.com/gregburd/ra.git /opt/ra
cd /opt/ra
cargo build --release -p ra-bench 2>&1 | tail -5

log "Setting up RAID storage"
# Detect NVMe drives (skip root)
NVME_DRIVES=$(lsblk -dpno NAME,TYPE | awk '$2=="disk" {print $1}' | grep nvme | grep -v nvme0n1 | head -4)
DRIVE_COUNT=$(echo "${NVME_DRIVES}" | wc -l)

if [[ "${DRIVE_COUNT}" -ge 2 ]]; then
    log "Setting up RAID-0 over ${DRIVE_COUNT} NVMe drives"
    mdadm --create /dev/md0 --level=0 --raid-devices="${DRIVE_COUNT}" ${NVME_DRIVES}
    mkfs.xfs /dev/md0
    mkdir -p /benchmark/workspace
    mount /dev/md0 /benchmark/workspace
    echo '/dev/md0 /benchmark/workspace xfs defaults,noatime 0 2' >> /etc/fstab
fi

log "Bootstrap complete — Ra is ready at /opt/ra"
BOOTSTRAP
)

# ---------------------------------------------------------------------------
# Step 5: Launch Intel instance
# ---------------------------------------------------------------------------
log "Step 5: Launching Intel instance (${INSTANCE_INTEL})"

LAUNCH_TEMPLATE_INTEL=$(cat <<JSON
{
    "ImageId": "${AMI_INTEL}",
    "InstanceType": "${INSTANCE_INTEL}",
    "KeyName": "${KEY_NAME}",
    "SecurityGroupIds": ["${SG_ID}"],
    "Placement": {"GroupName": "${PG_NAME}"},
    "UserData": "${USERDATA}",
    "BlockDeviceMappings": [
        {
            "DeviceName": "/dev/xvda",
            "Ebs": {
                "VolumeSize": 100,
                "VolumeType": "gp3",
                "Iops": 3000,
                "DeleteOnTermination": true
            }
        }
    ],
    "TagSpecifications": [
        {
            "ResourceType": "instance",
            "Tags": [
                {"Key": "Name", "Value": "${TAG_PREFIX}-intel"},
                {"Key": "Project", "Value": "ra-benchmark"},
                {"Key": "Architecture", "Value": "x86_64"}
            ]
        }
    ]
}
JSON
)

if [[ "${SPOT}" == "1" ]]; then
    INTEL_INSTANCE_ID=$(${AWS} ec2 request-spot-instances \
        --instance-count 1 \
        --type "one-time" \
        --launch-specification "${LAUNCH_TEMPLATE_INTEL}" \
        --query "SpotInstanceRequests[0].InstanceId" \
        --output text)
else
    INTEL_INSTANCE_ID=$(${AWS} ec2 run-instances \
        --cli-input-json "${LAUNCH_TEMPLATE_INTEL}" \
        --query "Instances[0].InstanceId" \
        --output text)
fi
log "  Intel instance: ${INTEL_INSTANCE_ID}"

# ---------------------------------------------------------------------------
# Step 6: Launch ARM instance
# ---------------------------------------------------------------------------
log "Step 6: Launching ARM instance (${INSTANCE_ARM})"

ARM_INSTANCE_ID=$(${AWS} ec2 run-instances \
    --image-id "${AMI_ARM}" \
    --instance-type "${INSTANCE_ARM}" \
    --key-name "${KEY_NAME}" \
    --security-group-ids "${SG_ID}" \
    --user-data "${USERDATA}" \
    --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=${TAG_PREFIX}-arm},{Key=Project,Value=ra-benchmark},{Key=Architecture,Value=arm64}]" \
    --query "Instances[0].InstanceId" \
    --output text)

log "  ARM instance: ${ARM_INSTANCE_ID}"

# ---------------------------------------------------------------------------
# Step 7: Wait for instances to be running
# ---------------------------------------------------------------------------
log "Step 7: Waiting for instances to reach running state"

${AWS} ec2 wait instance-running \
    --instance-ids "${INTEL_INSTANCE_ID}" "${ARM_INSTANCE_ID}"

INTEL_IP=$(${AWS} ec2 describe-instances \
    --instance-ids "${INTEL_INSTANCE_ID}" \
    --query "Reservations[0].Instances[0].PublicIpAddress" \
    --output text)

ARM_IP=$(${AWS} ec2 describe-instances \
    --instance-ids "${ARM_INSTANCE_ID}" \
    --query "Reservations[0].Instances[0].PublicIpAddress" \
    --output text)

# ---------------------------------------------------------------------------
# Step 8: Save instance info
# ---------------------------------------------------------------------------
cat > /tmp/ra-aws-instances.json <<JSON
{
    "region": "${REGION}",
    "intel": {
        "instance_id": "${INTEL_INSTANCE_ID}",
        "public_ip": "${INTEL_IP}",
        "type": "${INSTANCE_INTEL}",
        "arch": "x86_64"
    },
    "arm": {
        "instance_id": "${ARM_INSTANCE_ID}",
        "public_ip": "${ARM_IP}",
        "type": "${INSTANCE_ARM}",
        "arch": "arm64"
    },
    "security_group": "${SG_ID}",
    "placement_group": "${PG_NAME}"
}
JSON

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
cat <<EOF

============================================================
 PROVISIONING COMPLETE
============================================================

Intel instance: ${INTEL_INSTANCE_ID}  (${INTEL_IP})
ARM instance:   ${ARM_INSTANCE_ID}  (${ARM_IP})

Instance info saved to: /tmp/ra-aws-instances.json

Connect:
  ssh -i ~/.ssh/${KEY_NAME}.pem ec2-user@${INTEL_IP}   # Intel
  ssh -i ~/.ssh/${KEY_NAME}.pem ec2-user@${ARM_IP}     # ARM

Wait ~5 minutes for bootstrap to complete, then run:
  ssh ec2-user@${INTEL_IP} 'tail -20 /var/log/ra-bootstrap.log'

Run the benchmark suite (from Intel instance):
  ssh ec2-user@${INTEL_IP} 'cd /opt/ra && bash scripts/run-benchmark-suite.sh'

IMPORTANT: Stop instances when done to avoid charges:
  aws --region ${REGION} ec2 stop-instances \\
      --instance-ids ${INTEL_INSTANCE_ID} ${ARM_INSTANCE_ID}

Terminate when finished:
  aws --region ${REGION} ec2 terminate-instances \\
      --instance-ids ${INTEL_INSTANCE_ID} ${ARM_INSTANCE_ID}

============================================================
EOF
