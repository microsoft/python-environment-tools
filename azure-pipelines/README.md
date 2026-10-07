pre-release.yml: This should build from the `main` branch and publish to the Azure Pipeline feed. This will be consumed by extensions that are also doing pre-release builds. Signing is required on this build.

stable.yml: This builds tags matching `release/<year>.<build>`, where `<year>` is four digits and `<build>` is numeric, and publishes to the Azure Pipeline feed. The pipeline is triggered when a matching tag is pushed to GitHub. This will be consumed by extensions when publishing stable builds. Signing is required on this build.

playground.yml: This pipeline is for engineering/testing purposes so we can do fixes and tests without affecting the pipeline feeds. This will not publish to the Azure Pipeline feed. Signing is not required on this build.
