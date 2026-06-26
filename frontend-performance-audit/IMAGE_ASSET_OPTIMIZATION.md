# Image & Asset Optimization Guidelines

## 📸 Image Optimization Standards

### Format Selection Matrix

| Use Case | Format | Quality | Max Size | Notes |
|----------|--------|---------|----------|-------|
| Photos | WebP/AVIF | 80-85% | 150KB | Fallback to JPEG |
| Logos/Icons | SVG | - | 10KB | Inline for critical |
| Screenshots | WebP | 85% | 200KB | Lossless compression |
| Thumbnails | WebP | 75% | 30KB | Lazy load essential |
| Backgrounds | WebP | 70% | 100KB | Blur effect OK |
| Animations | Lottie/SVG | - | 50KB | Prefer CSS animations |

### Responsive Image Implementation

```jsx
// ✅ Good: Responsive image with multiple sizes
<picture>
  <source 
    type="image/webp"
    srcSet="
      /images/course-small.webp 480w,
      /images/course-medium.webp 768w,
      /images/course-large.webp 1200w
    "
    sizes="(max-width: 600px) 480px, (max-width: 900px) 768px, 1200px"
  />
  <img
    src="/images/course-fallback.jpg"
    alt="Course thumbnail"
    loading="lazy"
    decoding="async"
    width="1200"
    height="675"
  />
</picture>
```

### Critical Images (Above the Fold)

```jsx
// ✅ Good: Eager loading for critical images
<img
  src="/hero-image.webp"
  alt="Hero"
  loading="eager"
  fetchPriority="high"
  decoding="async"
/>
```

### Lazy Loading Implementation

```jsx
// ✅ Good: Native lazy loading
<img
  src="course-thumbnail.webp"
  alt="Course"
  loading="lazy"
  decoding="async"
/>

// ✅ Better: Intersection Observer for custom lazy loading
const LazyImage = ({ src, alt }) => {
  const [isLoaded, setIsLoaded] = useState(false);
  const [ref, isVisible] = useIntersectionObserver();

  return (
    <div ref={ref}>
      {isVisible && (
        <img
          src={src}
          alt={alt}
          onLoad={() => setIsLoaded(true)}
          style={{ opacity: isLoaded ? 1 : 0 }}
        />
      )}
    </div>
  );
};
```

---

## 🎨 SVG Optimization

### Optimization Checklist

- [ ] Remove unnecessary metadata and comments
- [ ] Minify paths and reduce precision
- [ ] Remove hidden elements
- [ ] Convert strokes to fills where possible
- [ ] Use viewBox for scalability
- [ ] Inline critical SVGs (< 2KB)
- [ ] Sprite sheet for repeated icons

### Before vs After

```xml
<!-- ❌ Unoptimized -->
<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24">
  <!-- Created with Adobe Illustrator -->
  <metadata>RDF metadata here...</metadata>
  <g>
    <path d="M12.000000,2.000000 L14.500000,8.750000 L..." />
  </g>
</svg>

<!-- ✅ Optimized -->
<svg viewBox="0 0 24 24" aria-hidden="true">
  <path d="M12 2l2.5 6.75L..."/>
</svg>
```

### React Component Pattern

```tsx
// ✅ Good: Reusable optimized SVG component
export const IconCheck = ({ size = 24, className = '' }) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    className={className}
    aria-hidden="true"
  >
    <path
      d="M20 6L9 17l-5-5"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);
```

---

## 🖼️ Image Processing Pipeline

### Build-Time Optimization

```javascript
// vite.config.js or next.config.js
module.exports = {
  images: {
    formats: ['image/avif', 'image/webp'],
    deviceSizes: [640, 750, 828, 1080, 1200],
    imageSizes: [16, 32, 48, 64, 96],
    minimumCacheTTL: 60,
  },
  
  webpack(config) {
    config.module.rules.push({
      test: /\.(png|jpe?g|gif|webp)$/i,
      use: [
        {
          loader: 'image-webpack-loader',
          options: {
            mozjpeg: { progressive: true, quality: 65 },
            pngquant: { quality: [0.65, 0.9], speed: 4 },
            gifsicle: { interlaced: false },
            webp: { quality: 75 },
          },
        },
      ],
    });
    return config;
  },
};
```

### Automated Compression Script

```javascript
// scripts/optimize-images.js
const sharp = require('sharp');
const glob = require('glob');
const path = require('path');
const fs = require('fs');

async function optimizeImages() {
  const images = glob.sync('public/images/**/*.{png,jpg,jpeg}');
  
  for (const image of images) {
    const outputPath = image.replace(/\.(png|jpg|jpeg)$/, '.webp');
    
    await sharp(image)
      .resize(1200, null, { withoutEnlargement: true })
      .webp({ quality: 80 })
      .toFile(outputPath);
    
    console.log(`Optimized: ${outputPath}`);
  }
}

optimizeImages();
```

---

## 📦 Font Optimization

### Font Loading Strategy

```css
/* ✅ Good: Font display swap with preload */
@font-face {
  font-family: 'Inter';
  src: url('/fonts/inter-var.woff2') format('woff2');
  font-weight: 100 900;
  font-display: swap;
  font-style: normal;
}

/* Preload critical fonts */
<link
  rel="preload"
  href="/fonts/inter-var.woff2"
  as="font"
  type="font/woff2"
  crossorigin="anonymous"
/>
```

### Variable Fonts Benefits

- **Single file** instead of multiple weights
- **Smaller total size** (typically 50-100KB vs 300KB+)
- **Flexible design** with continuous weight range

```css
/* ✅ Using variable font */
body {
  font-family: 'Inter var', sans-serif;
  font-weight: 400;
}

h1 {
  font-weight: 700;
}

.small {
  font-weight: 300;
}
```

---

## 🚀 Asset Delivery Optimization

### CDN Configuration

```javascript
// Cache headers for different asset types
const cacheHeaders = {
  // Immutable assets (with content hash)
  'assets/**': {
    'Cache-Control': 'public, max-age=31536000, immutable',
  },
  
  // HTML pages
  '**/*.html': {
    'Cache-Control': 'public, max-age=0, must-revalidate',
  },
  
  // API responses
  '/api/**': {
    'Cache-Control': 'private, no-cache',
  },
  
  // Images
  'images/**': {
    'Cache-Control': 'public, max-age=604800', // 1 week
  },
};
```

### Preloading Critical Assets

```html
<head>
  <!-- Preconnect to CDN -->
  <link rel="preconnect" href="https://cdn.chainverse.io" />
  
  <!-- Preload critical CSS -->
  <link rel="preload" href="/css/critical.css" as="style" />
  
  <!-- Preload hero image -->
  <link rel="preload" href="/images/hero.webp" as="image" />
  
  <!-- Prefetch next page resources -->
  <link rel="prefetch" href="/dashboard/main.js" />
</head>
```

---

## 📊 Performance Budget for Assets

### Size Limits by Asset Type

```json
{
  "budgets": [
    {
      "resourceType": "image",
      "maximumSize": "150kB",
      "maximumFileSize": "200kB"
    },
    {
      "resourceType": "font",
      "maximumSize": "50kB",
      "maximumFileSize": "75kB"
    },
    {
      "resourceType": "script",
      "maximumSize": "200kB",
      "maximumFileSize": "250kB"
    },
    {
      "resourceType": "stylesheet",
      "maximumSize": "50kB",
      "maximumFileSize": "75kB"
    }
  ]
}
```

### Monitoring Script

```javascript
// scripts/check-asset-sizes.js
const glob = require('glob');
const fs = require('fs');
const chalk = require('chalk');

const BUDGETS = {
  image: 150 * 1024, // 150KB
  font: 50 * 1024,   // 50KB
  script: 200 * 1024, // 200KB
};

function checkAssetSizes() {
  const images = glob.sync('public/images/**/*.{png,jpg,webp,gif}');
  const fonts = glob.sync('public/fonts/**/*.{woff,woff2}');
  const scripts = glob.sync('dist/**/*.js');
  
  let hasErrors = false;
  
  images.forEach(file => {
    const size = fs.statSync(file).size;
    if (size > BUDGETS.image) {
      console.error(chalk.red(`❌ ${file}: ${(size/1024).toFixed(1)}KB`));
      hasErrors = true;
    }
  });
  
  if (hasErrors) {
    console.error(chalk.yellow('\n⚠️  Some assets exceed size limits'));
    process.exit(1);
  } else {
    console.log(chalk.green('✅ All assets within budget'));
  }
}

checkAssetSizes();
```

---

## 🛠️ Tools & Libraries

### Essential Tools

1. **Sharp** - Image processing library
2. **SVGO** - SVG optimizer
3. **imagemin** - Batch image compression
4. **fontkit** - Font subsetting
5. **purgecss** - Remove unused CSS

### VS Code Extensions

- **Image preview** - See image previews
- **SVG Viewer** - Inline SVG preview
- **Webpack Bundle Analyzer** - Visual bundle analysis

### Online Tools

- **Squoosh.app** - Image optimization
- **SVGOMG** - SVG optimizer
- **Font Squirrel** - Font generator
- **TinyPNG** - PNG/JPEG compression

---

## ✅ Pre-Deployment Checklist

- [ ] All images converted to WebP/AVIF with fallbacks
- [ ] Responsive images implemented with srcset
- [ ] Lazy loading enabled for below-fold images
- [ ] Critical images use eager loading
- [ ] SVGs optimized with SVGO
- [ ] Variable fonts used where possible
- [ ] Font subsetting implemented
- [ ] CDN configured with proper caching
- [ ] Asset sizes within budget
- [ ] No oversized images (>200KB)
- [ ] Icons inlined or sprite-sheeted
- [ ] No unused assets in bundle
