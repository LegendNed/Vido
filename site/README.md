# vido.rip

Vido's website: one static page ([public/index.html](public/index.html)) served by Cloudflare
Workers as static assets. There's no build step; the only script, [src/index.js](src/index.js),
redirects `www.vido.rip` to `vido.rip` so Google indexes one copy of the page.

## Run it locally

```powershell
cd site
pnpm install
pnpm run dev     # http://localhost:8787
```

## Publish

1. Add **vido.rip** to your Cloudflare account (Websites → Add a domain) and point the domain's
   nameservers at Cloudflare. Wrangler can only attach a domain that's a zone in the same account.
2. Log in once and deploy:

   ```powershell
   cd site
   pnpm exec wrangler login
   pnpm run deploy
   ```

On deploy, Wrangler creates the DNS records and certificates for `vido.rip` and `www.vido.rip`
(`routes` in [wrangler.jsonc](wrangler.jsonc)). The `*.workers.dev` copy is turned off, so search
engines only see one site. `pnpm run check` validates the config without deploying.

## Search engines and AI crawlers

- [robots.txt](public/robots.txt) allows everyone and names the AI crawlers explicitly (GPTBot,
  ClaudeBot, PerplexityBot, Google-Extended and others). It also points to the
  [sitemap](public/sitemap.xml).
- [llms.txt](public/llms.txt) is a plain-text summary of Vido for AI assistants.
- The page has a canonical URL, Open Graph and Twitter tags for link previews, and JSON-LD
  (`SoftwareApplication`, `FAQPage` and `VideoObject`) so Google can show it as a free Windows app with FAQ
  answers.

**Cloudflare can block AI crawlers on your domain.** After adding vido.rip, open
**AI Crawl Control** (or Security → Bots) for the zone and make sure "Block AI bots" is off. Also
turn off Cloudflare's managed robots.txt there, or it will add disallow rules to this one.
Then submit `https://vido.rip/sitemap.xml` in Google Search Console and Bing Webmaster Tools.

## Changing it

- **Text and layout:** [public/index.html](public/index.html). The desktop layout comes first;
  the `@media` rules at the end of the `<style>` stack it for tablets and phones.
- **Screenshots:** `public/img/` (taken from the trailer's UI captures in `trailer/shots/`).
- **Hero video:** `public/video/vido-trailer.mp4`, a web copy (1600 px, about 5 MB) of
  `trailer/Vido-trailer.mp4`, plus `poster.jpg`. After re-rendering the trailer, run from the
  repo root:

  ```powershell
  third_party/ffmpeg/bin/ffmpeg.exe -i trailer/Vido-trailer.mp4 -vf scale=1600:-2:flags=lanczos -c:v libx264 -preset slow -crf 25 -pix_fmt yuv420p -c:a aac -b:a 128k -movflags +faststart site/public/video/vido-trailer.mp4
  ```
- **What AI assistants read:** [public/llms.txt](public/llms.txt). Keep it in step with the page.
- **After big changes:** update `<lastmod>` in [public/sitemap.xml](public/sitemap.xml).
