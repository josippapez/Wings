import type { SVGProps } from "react";

/**
 * The opencode mark: a square frame around a dimmed square. It follows `currentColor`, with the inner square
 * at 35% so it reads on light and dark themes.
 * Source: https://github.com/anomalyco/opencode/blob/dev/packages/console/app/src/asset/brand/opencode-logo-dark-square.svg
 * (MIT, Copyright (c) 2025 opencode, https://github.com/anomalyco/opencode/blob/dev/LICENSE). Paths are the
 * official ones, with the two fills replaced by currentColor.
 */
export function OpencodeIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 240 300" fill="currentColor" {...props}>
      <path d="M180 240H60V120H180V240Z" opacity={0.35} />
      <path d="M180 60H60V240H180V60ZM240 300H0V0H240V300Z" fillRule="evenodd" />
    </svg>
  );
}
