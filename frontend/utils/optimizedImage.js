/**
 * OptimizedImage component - FE-138
 *
 * Renders images optimized for mobile performance:
 * - Uses srcset + sizes for responsive image delivery
 * - Lazy loads images below the fold
 * - Applies aspect-ratio to prevent layout shift (CLS)
 * - Supports WebP with fallback via <picture>
 */

/**
 * @typedef {Object} OptimizedImageProps
 * @property {string} src        - Base image URL (jpg/png)
 * @property {string} [srcWebp] - WebP variant URL (optional)
 * @property {string} alt        - Alt text (required for accessibility)
 * @property {number} width      - Intrinsic width in px
 * @property {number} height     - Intrinsic height in px
 * @property {string} [sizes]   - Sizes attribute (default: responsive preset)
 * @property {boolean} [eager]  - Set true for above-the-fold images (disables lazy load)
 * @property {string} [className]
 */

/**
 * Generates a srcset string from a base URL for common breakpoints.
 * Expects the image host to support ?w= query param for resizing
 * (e.g. Cloudinary, Imgix, Next.js Image Optimization API).
 *
 * @param {string} src
 * @param {number[]} widths
 * @returns {string}
 */
export function buildSrcSet(src, widths = [320, 480, 640, 768, 1024, 1280]) {
  return widths.map((w) => `${src}?w=${w} ${w}w`).join(", ");
}

/**
 * Default sizes attribute — full-width on mobile, constrained on larger screens.
 */
export const DEFAULT_SIZES =
  "(max-width: 480px) 100vw, (max-width: 768px) 100vw, (max-width: 1024px) 50vw, 33vw";

/**
 * Returns the HTML string for an optimized <picture> element.
 * Use this in plain HTML / template engines.
 *
 * @param {OptimizedImageProps} props
 * @returns {string}
 */
export function optimizedImageHTML({
  src,
  srcWebp,
  alt,
  width,
  height,
  sizes = DEFAULT_SIZES,
  eager = false,
  className = "",
}) {
  const loading = eager ? "eager" : "lazy";
  const decoding = eager ? "sync" : "async";
  const srcset = buildSrcSet(src);
  const webpSrcset = srcWebp ? buildSrcSet(srcWebp) : buildSrcSet(src.replace(/\.(jpe?g|png)$/i, ".webp"));

  return `<picture>
  <source type="image/webp" srcset="${webpSrcset}" sizes="${sizes}" />
  <img
    src="${src}"
    srcset="${srcset}"
    sizes="${sizes}"
    alt="${alt}"
    width="${width}"
    height="${height}"
    loading="${loading}"
    decoding="${decoding}"
    ${className ? `class="${className}"` : ""}
    style="max-width:100%;height:auto;"
  />
</picture>`;
}
