import type { NextConfig } from 'next';

const nextConfig: NextConfig = {
  transpilePackages: ['@companyos/design-system', '@companyos/sdk'],
  reactStrictMode: true,
};

export default nextConfig;
