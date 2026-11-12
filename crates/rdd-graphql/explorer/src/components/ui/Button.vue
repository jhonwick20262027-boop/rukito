<script setup lang="ts">
import { computed } from "vue";
import { cva, type VariantProps } from "class-variance-authority";

const button = cva(
  "inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-md font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        primary:
          "[background-image:var(--grad-rust)] text-[#1a0d05] shadow-[0_2px_14px_rgba(255,90,31,0.32)] hover:[background-image:var(--grad-rust-hot)] hover:shadow-[0_4px_22px_rgba(255,90,31,0.48)]",
        secondary: "border border-line bg-subtle text-fg hover:border-line-strong hover:bg-inset",
        ghost: "text-muted hover:bg-subtle hover:text-fg",
        danger: "bg-danger text-white hover:brightness-110",
      },
      size: {
        sm: "h-7 px-2.5 text-xs",
        md: "h-8 px-3 text-[13px]",
        icon: "size-7",
      },
    },
    defaultVariants: { variant: "secondary", size: "md" },
  },
);

type Variants = VariantProps<typeof button>;

interface Props {
  variant?: Variants["variant"];
  size?: Variants["size"];
  type?: "button" | "submit" | "reset";
  disabled?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  variant: "secondary",
  size: "md",
  type: "button",
  disabled: false,
});

const classes = computed(() => button({ variant: props.variant, size: props.size }));
</script>

<template>
  <button :type="props.type" :disabled="props.disabled" :class="classes">
    <slot />
  </button>
</template>
