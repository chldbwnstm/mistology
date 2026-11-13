const TRUSTED_PROXY_HOPS = 2;
const ADMIN_PREFIX = "10.240.";

export function verifyAdminCidr(xffHeader: string | null): boolean {
  if (!xffHeader) {
    return false;
  }
  const chain = xffHeader
    .split(",")
    .map((ip) => ip.trim())
    .filter(Boolean);
  if (chain.length < TRUSTED_PROXY_HOPS) {
    throw new Error(
      "AdminCidrProxyError: x-forwarded-for spoofed without trusted hop count"
    );
  }
  const clientIp = chain[chain.length - TRUSTED_PROXY_HOPS];
  return clientIp.startsWith(ADMIN_PREFIX);
}
