/// <reference path="./.sst/platform/config.d.ts" />

/**
 * SST v3 Infrastructure Configuration for VerityDB
 *
 * Resources:
 * - ECR repository for container images
 * - App Runner service (auto-scaling, pay-per-use)
 * - CloudFront distribution (production only)
 * - Route 53 DNS records (production only)
 *
 * Usage:
 *   npx sst deploy --stage dev        # Just App Runner, no custom domain
 *   npx sst deploy --stage production # Full stack with CloudFront + DNS
 */

export default $config({
  app(input) {
    return {
      name: "veritydb",
      removal: input?.stage === "production" ? "retain" : "remove",
      protect: ["production"].includes(input?.stage),
      home: "aws",
      providers: {
        aws: {
          region: "ap-southeast-2", // Sydney
        },
      },
    };
  },
  async run() {
    const isProduction = $app.stage === "production";

    // ECR Repository for container images
    const repo = new aws.ecr.Repository("SiteRepo", {
      name: `veritydb-site-${$app.stage}`,
      forceDelete: !isProduction,
      imageScanningConfiguration: {
        scanOnPush: true,
      },
      imageTagMutability: "MUTABLE",
    });

    // ECR Lifecycle policy to clean up old images
    new aws.ecr.LifecyclePolicy("SiteRepoLifecycle", {
      repository: repo.name,
      policy: JSON.stringify({
        rules: [
          {
            rulePriority: 1,
            description: "Keep last 10 images",
            selection: {
              tagStatus: "any",
              countType: "imageCountMoreThan",
              countNumber: 10,
            },
            action: {
              type: "expire",
            },
          },
        ],
      }),
    });

    // App Runner IAM Role
    const appRunnerRole = new aws.iam.Role("AppRunnerRole", {
      assumeRolePolicy: JSON.stringify({
        Version: "2012-10-17",
        Statement: [
          {
            Effect: "Allow",
            Principal: {
              Service: "build.apprunner.amazonaws.com",
            },
            Action: "sts:AssumeRole",
          },
        ],
      }),
    });

    // Allow App Runner to pull from ECR
    new aws.iam.RolePolicyAttachment("AppRunnerEcrPolicy", {
      role: appRunnerRole.name,
      policyArn:
        "arn:aws:iam::aws:policy/service-role/AWSAppRunnerServicePolicyForECRAccess",
    });

    // Auto-scaling configuration
    const autoScaling = new aws.apprunner.AutoScalingConfigurationVersion(
      "SiteAutoScaling",
      {
        autoScalingConfigurationName: `veritydb-site-${$app.stage}`,
        minSize: 1, // Sydney region requires minimum 1
        maxSize: 2,
        maxConcurrency: 100,
      }
    );

    // App Runner Service
    const appRunner = new aws.apprunner.Service("Site", {
      serviceName: `veritydb-site-${$app.stage}`,
      sourceConfiguration: {
        authenticationConfiguration: {
          accessRoleArn: appRunnerRole.arn,
        },
        imageRepository: {
          imageIdentifier: $interpolate`${repo.repositoryUrl}:latest`,
          imageRepositoryType: "ECR",
          imageConfiguration: {
            port: "3000",
            runtimeEnvironmentVariables: {
              RUST_LOG: "info",
            },
          },
        },
        autoDeploymentsEnabled: false,
      },
      instanceConfiguration: {
        cpu: "256",
        memory: "512",
      },
      healthCheckConfiguration: {
        protocol: "HTTP",
        path: "/",
        interval: 10,
        timeout: 5,
        healthyThreshold: 1,
        unhealthyThreshold: 5,
      },
      autoScalingConfigurationArn: autoScaling.arn,
    });

    // For dev: just return App Runner URL
    // For production: add CloudFront + Route 53 (requires hosted zone setup first)
    if (!isProduction) {
      return {
        url: appRunner.serviceUrl,
        ecrRepo: repo.repositoryUrl,
      };
    }

    // --- Production only: CloudFront + Route 53 ---

    const domain = "www.veritydb.com";

    // Provider for us-east-1 (required for CloudFront ACM certificates)
    const usEast1 = new aws.Provider("us-east-1", { region: "us-east-1" });

    // ACM Certificate (must be in us-east-1 for CloudFront)
    const certificate = new aws.acm.Certificate(
      "SiteCert",
      {
        domainName: domain,
        validationMethod: "DNS",
      },
      { provider: usEast1 }
    );

    // CloudFront Distribution
    const cdn = new aws.cloudfront.Distribution("SiteCdn", {
      enabled: true,
      aliases: [domain],
      defaultRootObject: "",
      priceClass: "PriceClass_All",

      origins: [
        {
          domainName: appRunner.serviceUrl.apply((url) =>
            url.replace("https://", "")
          ),
          originId: "apprunner",
          customOriginConfig: {
            httpPort: 80,
            httpsPort: 443,
            originProtocolPolicy: "https-only",
            originSslProtocols: ["TLSv1.2"],
          },
        },
      ],

      defaultCacheBehavior: {
        targetOriginId: "apprunner",
        viewerProtocolPolicy: "redirect-to-https",
        allowedMethods: ["GET", "HEAD", "OPTIONS"],
        cachedMethods: ["GET", "HEAD"],
        compress: true,
        cachePolicyId: "658327ea-f89d-4fab-a63d-7e88639e58f6",
        originRequestPolicyId: "b689b0a8-53d0-40ab-baf2-68738e2966ac",
      },

      orderedCacheBehaviors: [
        {
          pathPattern: "/css/*",
          targetOriginId: "apprunner",
          viewerProtocolPolicy: "redirect-to-https",
          allowedMethods: ["GET", "HEAD"],
          cachedMethods: ["GET", "HEAD"],
          compress: true,
          cachePolicyId: "658327ea-f89d-4fab-a63d-7e88639e58f6",
          originRequestPolicyId: "b689b0a8-53d0-40ab-baf2-68738e2966ac",
        },
        {
          pathPattern: "/vendor/*",
          targetOriginId: "apprunner",
          viewerProtocolPolicy: "redirect-to-https",
          allowedMethods: ["GET", "HEAD"],
          cachedMethods: ["GET", "HEAD"],
          compress: true,
          cachePolicyId: "658327ea-f89d-4fab-a63d-7e88639e58f6",
          originRequestPolicyId: "b689b0a8-53d0-40ab-baf2-68738e2966ac",
        },
      ],

      restrictions: {
        geoRestriction: {
          restrictionType: "none",
        },
      },

      viewerCertificate: {
        acmCertificateArn: certificate.arn,
        sslSupportMethod: "sni-only",
        minimumProtocolVersion: "TLSv1.2_2021",
      },
    });

    // Route 53 DNS Record
    const hostedZone = aws.route53.getZone({
      name: "veritydb.com",
    });

    new aws.route53.Record("SiteDns", {
      zoneId: hostedZone.then((z) => z.zoneId),
      name: domain,
      type: "A",
      aliases: [
        {
          name: cdn.domainName,
          zoneId: cdn.hostedZoneId,
          evaluateTargetHealth: false,
        },
      ],
    });

    // Certificate DNS validation record
    const certValidation = certificate.domainValidationOptions.apply(
      (options) => options[0]
    );

    new aws.route53.Record("SiteCertValidation", {
      zoneId: hostedZone.then((z) => z.zoneId),
      name: certValidation.resourceRecordName,
      type: certValidation.resourceRecordType,
      records: [certValidation.resourceRecordValue],
      ttl: 300,
    });

    return {
      url: `https://${domain}`,
      appRunnerUrl: appRunner.serviceUrl,
      cdnUrl: $interpolate`https://${cdn.domainName}`,
      ecrRepo: repo.repositoryUrl,
    };
  },
});
