# Forgejo Actions Workflows

This directory contains CI/CD workflows for Codeberg using Forgejo Actions.

## Available Workflows

### deploy-pages.yml

Automatically builds and deploys documentation to Codeberg Pages when changes are pushed to the `main` branch.

**Triggers:**
- Push to `main` branch
- Changes in: `docs/`, `crates/`, `rules/`, or workflow file itself

**What it does:**
1. Checks out repository
2. Sets up Node.js 22 and Rust toolchain
3. Builds WASM documentation module (if present)
4. Builds VitePress documentation
5. Builds Rust API documentation (rustdoc)
6. Combines all documentation
7. Pushes to `pages` branch for Codeberg Pages hosting

**Requirements:**
- Repository secret: `PAGES_TOKEN` (Codeberg access token with repository scope)
- Forgejo Actions enabled in repository settings

**Manual trigger:**
```bash
# Push to main will automatically trigger deployment
git push origin main
```

## About Forgejo Actions

Forgejo Actions is similar to GitHub Actions but with some differences:

- Workflow files location: `.forgejo/workflows/` (not `.github/workflows/`)
- Syntax is partially compatible with GitHub Actions
- Requires self-hosted runners or Codeberg's shared runners
- Documentation: https://forgejo.org/docs/latest/user/actions/

## Setup Instructions

1. **Enable Actions in repository**:
   - Settings → Units → Enable "Actions"

2. **Create access token**:
   - User Settings → Applications → Create token
   - Scope: `repository` (read/write)
   - Copy token value

3. **Add token as secret**:
   - Repository Settings → Secrets and Variables → Actions
   - Secret name: `PAGES_TOKEN`
   - Secret value: [paste token]

4. **Push workflow file**:
   ```bash
   git add .forgejo/workflows/deploy-pages.yml
   git commit -m "Add Codeberg Pages deployment workflow"
   git push origin main
   ```

5. **Verify deployment**:
   - Check Actions tab for workflow runs
   - Verify `pages` branch is created
   - Visit: https://[username].codeberg.page/ra/

## Differences from GitHub Actions

| Feature | GitHub Actions | Forgejo Actions |
|---------|---------------|-----------------|
| Workflow location | `.github/workflows/` | `.forgejo/workflows/` |
| Runner | GitHub-hosted | Self-hosted or Codeberg shared |
| Syntax | GitHub Actions YAML | Partially compatible |
| Marketplace actions | Full support | Limited support |
| Runner availability | Always available | May require request |

## Troubleshooting

**Workflow not running:**
- Verify Actions are enabled in Settings → Units
- Check branch filter matches your push
- Look for errors in Actions tab

**Build failures:**
- Review logs in Actions tab
- Verify all required tools are available in runner
- Check secret configuration

**Pages not updating:**
- Verify `pages` branch is created
- Check `pages` branch has content
- Visit repository settings to confirm Pages configuration

## Additional Resources

- [Forgejo Actions Documentation](https://forgejo.org/docs/latest/user/actions/)
- [Codeberg Pages Documentation](https://docs.codeberg.org/codeberg-pages/)
- [Deployment Guide](../../docs/deployment.md)
