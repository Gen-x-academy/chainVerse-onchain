/**
 * Webpack Performance Optimization Configuration
 * 
 * This configuration file implements:
 * - Code splitting and chunk optimization
 * - Tree shaking and dead code elimination
 * - Bundle size optimization
 * - Compression with Brotli and Gzip
 * - Module concatenation
 */

const path = require('path');
const webpack = require('webpack');
const BundleAnalyzerPlugin = require('webpack-bundle-analyzer').BundleAnalyzerPlugin;
const CompressionPlugin = require('compression-webpack-plugin');
const TerserPlugin = require('terser-webpack-plugin');
const CssMinimizerPlugin = require('css-minimizer-webpack-plugin');
const { WebpackManifestPlugin } = require('webpack-manifest-plugin');
const SpeedMeasurePlugin = require('speed-measure-webpack-plugin');

const isProduction = process.env.NODE_ENV === 'production';
const analyzeBundle = process.env.ANALYZE_BUNDLE === 'true';

// Performance budgets
const performanceBudgets = {
  initialPageSize: 200 * 1024, // 200KB
  totalPageSize: 500 * 1024,   // 500KB
  maxRequests: 50,
  maxCriticalRequests: 10,
};

const config = {
  mode: isProduction ? 'production' : 'development',
  
  entry: {
    main: './src/index.tsx',
    vendor: ['react', 'react-dom'],
  },
  
  output: {
    path: path.resolve(__dirname, 'dist'),
    filename: isProduction ? 'js/[name].[contenthash:8].js' : 'js/[name].js',
    chunkFilename: isProduction ? 'js/[name].[contenthash:8].chunk.js' : 'js/[name].chunk.js',
    assetModuleFilename: 'assets/[hash][ext][query]',
    clean: true,
    publicPath: '/',
  },
  
  resolve: {
    extensions: ['.ts', '.tsx', '.js', '.jsx'],
    alias: {
      '@components': path.resolve(__dirname, 'src/components'),
      '@hooks': path.resolve(__dirname, 'src/hooks'),
      '@utils': path.resolve(__dirname, 'src/utils'),
      '@services': path.resolve(__dirname, 'src/services'),
      // Pre-built optimized versions of heavy libraries
      'react-dom$': 'react-dom/profiling',
    },
  },
  
  module: {
    rules: [
      // TypeScript with babel for optimal tree-shaking
      {
        test: /\.tsx?$/,
        use: {
          loader: 'babel-loader',
          options: {
            presets: [
              '@babel/preset-env',
              '@babel/preset-typescript',
              ['@babel/preset-react', { runtime: 'automatic' }],
            ],
            plugins: [
              // Transform imports for better tree-shaking
              ['import', { libraryName: 'lodash', camel2DashComponentName: false }],
            ],
            cacheDirectory: true,
            cacheCompression: false,
          },
        },
        exclude: /node_modules/,
      },
      
      // CSS with PostCSS and PurgeCSS
      {
        test: /\.css$/,
        use: [
          'style-loader',
          {
            loader: 'css-loader',
            options: {
              modules: {
                auto: true,
                localIdentName: isProduction ? '[hash:base64:8]' : '[path][name]__[local]',
              },
              importLoaders: 1,
            },
          },
          {
            loader: 'postcss-loader',
            options: {
              postcssOptions: {
                plugins: [
                  'tailwindcss',
                  'autoprefixer',
                  ...(isProduction ? ['cssnano'] : []),
                ],
              },
            },
          },
        ],
      },
      
      // Image optimization
      {
        test: /\.(png|jpe?g|gif|webp)$/i,
        type: 'asset',
        parser: { dataUrlCondition: { maxSize: 8 * 1024 } }, // 8KB inline limit
        generator: {
          filename: 'images/[hash][ext][query]',
        },
      },
      
      // SVG optimization
      {
        test: /\.svg$/,
        use: [
          {
            loader: '@svgr/webpack',
            options: {
              svgo: true,
              svgoConfig: {
                plugins: [
                  {
                    name: 'preset-default',
                    params: {
                      overrides: {
                        removeViewBox: false,
                        mergePaths: false,
                        convertShapeToPath: false,
                      },
                    },
                  },
                  'removeDimensions',
                ],
              },
            },
          },
        ],
      },
      
      // Font optimization
      {
        test: /\.(woff|woff2|eot|ttf|otf)$/i,
        type: 'asset/resource',
        generator: {
          filename: 'fonts/[hash][ext][query]',
        },
      },
    ],
    
    // Optimize node_modules parsing
    unsafeCache: true,
  },
  
  optimization: {
    minimize: isProduction,
    minimizer: [
      new TerserPlugin({
        parallel: true,
        terserOptions: {
          compress: {
            drop_console: isProduction,
            pure_funcs: isProduction ? ['console.log', 'console.debug'] : [],
          },
          format: {
            comments: false,
          },
        },
        extractComments: false,
      }),
      new CssMinimizerPlugin({
        parallel: true,
        minimizerOptions: {
          preset: [
            'default',
            {
              discardComments: { removeAll: true },
              reduceIdents: true,
              zindex: true,
            },
          ],
        },
      }),
    ],
    
    // Code splitting strategy
    splitChunks: {
      chunks: 'all',
      cacheGroups: {
        // Vendor chunks for node_modules
        vendors: {
          test: /[\\/]node_modules[\\/]/,
          name: 'vendors',
          priority: 10,
          enforce: true,
        },
        
        // React chunk
        react: {
          test: /[\\/]node_modules[\\/](react|react-dom)[\\/]/,
          name: 'react',
          priority: 20,
        },
        
        // Common chunks
        common: {
          minChunks: 2,
          name: 'common',
          priority: 5,
          reuseExistingChunk: true,
        },
        
        // Async chunks for lazy-loaded components
        async: {
          chunks: 'async',
          minSize: 20 * 1024, // 20KB minimum
          name: 'async',
          priority: 0,
        },
      },
    },
    
    // Runtime chunk
    runtimeChunk: {
      name: 'runtime',
    },
    
    // Module concatenation for smaller bundles
    concatenateModules: isProduction,
    
    // Remove unused exports
    usedExports: true,
    
    // Side effects detection
    sideEffects: true,
    
    // Chunk size limits
    maxInitialRequestSize: performanceBudgets.initialPageSize,
    maxAsyncRequestSize: performanceBudgets.initialPageSize,
  },
  
  plugins: [
    // Environment variables
    new webpack.EnvironmentPlugin({
      NODE_ENV: isProduction ? 'production' : 'development',
    }),
    
    // Manifest for cache busting
    new WebpackManifestPlugin({
      fileName: 'manifest.json',
      publicPath: '/',
    }),
    
    // Compression plugins
    ...(isProduction ? [
      // Brotli compression (better than gzip)
      new CompressionPlugin({
        algorithm: 'brotliCompress',
        test: /\.(js|css|html|svg|json)$/,
        threshold: 10240, // Only compress files > 10KB
        minRatio: 0.8, // Only compress if compressed size < 80% of original
        deleteOriginalAssets: false,
      }),
      
      // Gzip fallback
      new CompressionPlugin({
        algorithm: 'gzip',
        test: /\.(js|css|html|svg|json)$/,
        threshold: 10240,
        minRatio: 0.8,
        deleteOriginalAssets: false,
      }),
    ] : []),
    
    // Bundle analyzer (optional)
    ...(analyzeBundle ? [
      new BundleAnalyzerPlugin({
        analyzerMode: 'static',
        openAnalyzer: true,
        reportFilename: 'bundle-report.html',
      }),
    ] : []),
    
    // Build performance monitoring
    new SpeedMeasurePlugin(),
  ],
  
  performance: {
    hints: isProduction ? 'warning' : false,
    maxEntrypointSize: performanceBudgets.initialPageSize,
    maxAssetSize: performanceBudgets.initialPageSize,
    assetFilter: (assetFilename) => {
      // Only check JS and CSS files
      return assetFilename.endsWith('.js') || assetFilename.endsWith('.css');
    },
  },
  
  stats: {
    all: false,
    assets: true,
    chunks: false,
    modules: false,
    timings: true,
    builtAt: true,
    errors: true,
    warnings: true,
  },
  
  infrastructureLogging: {
    level: 'warn',
  },
  
  // DevServer optimization
  devServer: isProduction ? {} : {
    hot: true,
    liveReload: true,
    client: {
      progress: true,
      overlay: {
        errors: true,
        warnings: false,
      },
    },
    compress: true,
    port: 3000,
    historyApiFallback: true,
  },
};

module.exports = config;
