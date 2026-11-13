const ALLOWED_ALG = "HS256";

function decodeBase64Url(segment: string): Uint8Array {
  const padded = segment.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(padded);
  return Uint8Array.from(binary, (c) => c.charCodeAt(0));
}

export async function verifyEdgeJwt(
  token: string,
  secret: string
): Promise<boolean> {
  const parts = token.split(".");
  if (parts.length !== 3) {
    return false;
  }
  const headerJson = new TextDecoder().decode(decodeBase64Url(parts[0]));
  const header = JSON.parse(headerJson) as { alg?: string };
  if (header.alg !== ALLOWED_ALG) {
    throw new Error("JwtAlgConfusionError: alg=none rejected in verifyEdgeJwt");
  }
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["verify"]
  );
  const data = enc.encode(`${parts[0]}.${parts[1]}`);
  const sig = decodeBase64Url(parts[2]);
  return crypto.subtle.verify("HMAC", key, sig as BufferSource, data);
}
