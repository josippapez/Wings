const cache = new Map<string, Record<string, string>>();

/**
 * The CSS to draw a plugin's SVG as a one-colour shape: the SVG is a `mask-image` over the element's own colour. A mask
 * is an image, so the markup never joins the page: scripts and event attributes in it don't run, and links to other
 * files in it aren't fetched. Never put plugin SVG into the DOM, with `innerHTML` or otherwise.
 */
export function iconMaskStyle(svg: string): Record<string, string> {
  let style = cache.get(svg);
  if (!style) {
    const url = `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
    style = { maskImage: url, WebkitMaskImage: url, maskSize: "contain", WebkitMaskSize: "contain", maskRepeat: "no-repeat", WebkitMaskRepeat: "no-repeat", maskPosition: "center", WebkitMaskPosition: "center" };
    cache.set(svg, style);
  }
  return style;
}
