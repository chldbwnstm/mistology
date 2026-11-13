export interface NumericColumnConfig {
  precision: number;
  scale: number;
  mode: "string";
}

export const ordersTable = {
  tableName: "orders",
  columns: {
    id: { type: "uuid", primaryKey: true },
    totalAmount: {
      precision: 20,
      scale: 4,
      mode: "string",
    } satisfies NumericColumnConfig,
  },
} as const;
