/** @type {import('next').NextConfig} */
const nextConfig = {
  output: "standalone",
  env: {
    NEXT_PUBLIC_HUB_URL:
      process.env.NEXT_PUBLIC_HUB_URL ||
      process.env.HUB_URL ||
      "http://localhost:3000",
    NEXT_PUBLIC_AGENT_URL:
      process.env.NEXT_PUBLIC_AGENT_URL ||
      process.env.AGENT_URL ||
      "http://localhost:8000",
  },
};

module.exports = nextConfig;
