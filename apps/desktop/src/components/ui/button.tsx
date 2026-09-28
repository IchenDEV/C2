import { Button as ButtonPrimitive } from "@base-ui/react/button";
import { cva } from "class-variance-authority";
import type { VariantProps } from "class-variance-authority";
import * as React from "react";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "gap-module-inset rounded-control text-body duration-feedback focus-visible:border-ring aria-invalid:border-destructive aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 [&_svg:not([class*='size-'])]:size-icon-control inline-flex shrink-0 items-center justify-center font-medium whitespace-nowrap transition-colors outline-none disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default:
          "bg-primary text-primary-foreground hover:bg-primary-hover active:bg-primary-pressed",
        destructive:
          "bg-destructive text-destructive-foreground hover:bg-destructive-hover",
        outline:
          "bg-fill-rest text-foreground hover:bg-fill-hover active:bg-fill-pressed",
        secondary:
          "bg-fill-rest text-foreground hover:bg-fill-hover active:bg-fill-pressed",
        ghost:
          "hover:bg-fill-hover hover:text-foreground active:bg-fill-pressed",
        link: "text-primary underline-offset-4 hover:underline",
        selectable:
          "text-foreground hover:bg-fill-hover data-[selected=true]:bg-fill-selected data-[selected=true]:hover:bg-fill-selected-hover active:bg-fill-pressed data-[selected=true]:active:bg-fill-pressed relative bg-transparent before:absolute before:inset-y-2 before:left-0.5 before:w-0.5 before:rounded-full before:bg-current before:opacity-0 data-[selected=true]:before:opacity-100 disabled:data-[selected=true]:opacity-100",
      },
      size: {
        default: "h-control px-4 py-2 has-[>svg]:px-3",
        compact:
          "h-control-mini gap-inline px-surface-inset has-[>svg]:px-module-inset",
        field:
          "h-control-field gap-inline px-surface-inset has-[>svg]:px-module-inset",
        xs: "h-control-mini rounded-control text-metadata gap-1 px-2 has-[>svg]:px-1.5 [&_svg:not([class*='size-'])]:size-3",
        sm: "h-control-mini gap-inline px-surface-inset has-[>svg]:px-module-inset",
        lg: "h-control-field rounded-control px-6 has-[>svg]:px-4",
        icon: "size-control rounded-full",
        "icon-xs":
          "size-control-mini rounded-full [&_svg:not([class*='size-'])]:size-3",
        "icon-sm": "size-control rounded-full",
        "icon-lg": "size-control-field rounded-full",
        row: "min-h-navigation-row gap-module-inset px-module-inset py-control-group [&_[data-slot=navigation-row-meta]]:text-callout [&_[data-slot=navigation-row-meta]]:text-muted-foreground [&_[data-slot=selectable-row-description]]:text-callout [&_[data-slot=selectable-row-description]]:text-muted-foreground [&_[data-slot=selectable-row-meta]]:text-callout [&_[data-slot=selectable-row-meta]]:text-muted-foreground w-full justify-start text-start font-normal whitespace-normal",
      },
      focusStyle: {
        default: "focus-visible:focus-ring",
        inset: "focus-visible:focus-ring-inset",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
      focusStyle: "default",
    },
  }
);

const Button = React.forwardRef<
  HTMLButtonElement,
  ButtonPrimitive.Props & VariantProps<typeof buttonVariants>
>(
  (
    {
      className,
      variant = "default",
      size = "default",
      focusStyle = "default",
      ...props
    },
    ref
  ) => {
    return (
      <ButtonPrimitive
        ref={ref}
        data-slot="button"
        data-variant={variant}
        data-size={size}
        className={cn(buttonVariants({ variant, size, focusStyle, className }))}
        {...props}
      />
    );
  }
);
Button.displayName = "Button";

export { Button, buttonVariants };
