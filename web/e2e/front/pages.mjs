import http from "node:http";

const removed =
  /^(cookie|authorization|host|connection|keep-alive|transfer-encoding|te|trailer|upgrade|proxy-.*|x-forwarded-.*)$/i;

/** Mirrors the page boundary: browser credentials terminate at the front, never at Next.js. */
export function page(request, response) {
  if (!["GET", "HEAD"].includes(request.method)) {
    response.writeHead(405);
    response.end();
    return;
  }
  const headers = Object.fromEntries(
    Object.entries(request.headers).filter(([name]) => !removed.test(name)),
  );
  headers["x-forwarded-host"] = "localhost:3100";
  headers["x-forwarded-proto"] = "http";
  const upstream = http.request(
    {
      hostname: "localhost",
      port: 3101,
      method: request.method,
      path: request.url,
      headers,
      timeout: 30_000,
    },
    (incoming) => {
      const forwarded = Object.fromEntries(
        Object.entries(incoming.headers).filter(
          ([name]) =>
            !/^(set-cookie|connection|transfer-encoding)$/i.test(name),
        ),
      );
      response.writeHead(incoming.statusCode, {
        ...forwarded,
        "x-content-type-options": "nosniff",
        "referrer-policy": "same-origin",
        "x-frame-options": "DENY",
      });
      incoming.pipe(response);
      incoming.on("error", () => response.destroy());
    },
  );
  upstream.on("timeout", () => upstream.destroy());
  upstream.on("error", () => {
    if (!response.headersSent) response.writeHead(502);
    response.end("The web interface is not available yet.");
  });
  response.on("close", () => upstream.destroy());
  request.pipe(upstream);
}
