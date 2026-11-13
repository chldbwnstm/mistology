import { verifyEdgeJwt } from "../../../../packages/auth/src/jwt";

export interface EdgeRequest {
  url: string;
  headers: Headers;
}

export async function middleware(req: EdgeRequest): Promise<Response | null> {
  const url = new URL(req.url);
  if (url.pathname.startsWith("/login") || url.pathname.startsWith("/_next")) {
    return null;
  }
  const auth = req.headers.get("authorization") ?? "";
  const token = auth.startsWith("Bearer ") ? auth.slice(7) : "";
  const secret = process.env.EDGE_JWT_SECRET ?? "";
  if (!token || !(await verifyEdgeJwt(token, secret))) {
    return new Response("Unauthorized", { status: 401 });
  }
  return null;
}
