import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  // Required by infrastructure/docker/Dockerfile.web (staging image).
  output: 'standalone',
  transpilePackages: ['@companyos/design-system', '@companyos/sdk'],
  reactStrictMode: true,
};

export default nextConfig;
