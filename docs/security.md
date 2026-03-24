# Documentation Security Notes

## NPM Audit Findings

### Current Status

```bash
$ npm audit
# 4 moderate severity vulnerabilities in dev dependencies
```

### esbuild Development Server Vulnerability (GHSA-67mh-4wv8-2f99)

**Severity**: Moderate
**Scope**: Development only
**Status**: Accepted risk

#### Description

The esbuild development server accepts requests from any origin during local development. This is a design choice for developer convenience and only affects:
- Local development environments (`npm run dev`)
- Not production builds (`npm run build`)
- Not deployed documentation

#### Risk Assessment

- **Impact**: Low - Only affects developers running `npm run dev` locally
- **Likelihood**: Low - Requires attacker to already have access to developer's network
- **Mitigation**: Use production build for deployment; dev server is localhost-only by default

#### Resolution

This is an **accepted risk** because:
1. VitePress documentation is served statically in production (no esbuild dev server)
2. The dev server is only used during local development
3. Blocking cross-origin requests would break hot module replacement (HMR)
4. Developers should use trusted networks when running dev servers

#### For Production Deployment

Always use the build command, which produces static HTML/CSS/JS:
```bash
npm run build:docs  # Generates static site in .vitepress/dist/
```

The static output has no security vulnerabilities and can be served by any web server.

### Future Actions

Monitor VitePress and esbuild releases for security updates:
- VitePress: https://github.com/vuejs/vitepress/releases
- esbuild: https://github.com/evanw/esbuild/releases

Update dependencies when fixes are available:
```bash
npm update vitepress
```

## Security Best Practices

### Development Environment

- Run dev servers only on trusted networks
- Use `npm run dev` only on localhost
- Never expose development servers to the internet

### Production Deployment

- Always use `npm run build:docs` for production
- Serve static files from `.vitepress/dist/`
- Use CDN or static hosting (GitHub Pages, Netlify, Vercel, etc.)
- Enable HTTPS in production

## Reporting Security Issues

If you discover a security vulnerability in the Ra documentation or build process:

1. **Do not** open a public GitHub issue
2. Email security concerns to: [maintainer email]
3. Include:
   - Description of the vulnerability
   - Steps to reproduce
   - Potential impact
   - Suggested fix (if available)

We will respond within 48 hours and work with you to address the issue.
