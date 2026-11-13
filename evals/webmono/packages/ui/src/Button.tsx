export interface ButtonProps {
  label: string;
  disabled?: boolean;
}

export function Button({ label, disabled = false }: ButtonProps) {
  return {
    type: "button",
    props: {
      disabled,
      className: "inline-flex items-center rounded px-3 py-1.5",
      children: label,
    },
  };
}
