import { readPublicEnv } from "@/lib/env";

export interface CheckoutProps {
  orderId: string;
  amountFormatted: string;
}

export function CheckoutPage({ orderId, amountFormatted }: CheckoutProps) {
  const { apiOrigin } = readPublicEnv();
  const actionUrl = `${apiOrigin}/v1/orders/${encodeURIComponent(orderId)}/confirm`;

  return {
    type: "main",
    props: {
      "data-action": actionUrl,
      children: `Confirm order ${orderId} (${amountFormatted})`,
    },
  };
}
