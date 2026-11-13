export interface PublicEnv {
  apiOrigin: string;
}

export function readPublicEnv(): PublicEnv {
  const raw = process.env.NEXT_PUBLIC_API_ORIGIN;
  if (!raw || !raw.startsWith("https://")) {
    throw new Error(
      "HydrationEnvError: server/client mismatch on NEXT_PUBLIC_API_ORIGIN"
    );
  }
  return {
    apiOrigin: raw.replace(/\/+$/, ""),
  };
}
