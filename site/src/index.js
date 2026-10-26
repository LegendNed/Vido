// Sends www.vido.rip to vido.rip with a permanent redirect, so search engines see one site
// instead of two copies of the page; everything else is served from public/.
export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.hostname === "www.vido.rip") {
      url.hostname = "vido.rip";
      return Response.redirect(url.toString(), 301);
    }
    return env.ASSETS.fetch(request);
  },
};
