/** @type {import('next').NextConfig} */
const nextConfig = {
  /* RATA renders no <Image>, so Next's image optimizer is pure attack surface:
     an unauthenticated endpoint carrying a live critical advisory
     (GHSA-2xp9-vwfh-vxw4, remote code execution through AVIF decoding) plus a
     run of DoS and SSRF ones, none of which have a fix short of a major
     upgrade. `unoptimized` takes the optimizer out of the build, and it is
     handled before middleware so it cannot be closed from there.

     formats and remotePatterns are set too: belt and braces if a future page
     ever does render an image and turns the optimizer back on — no AVIF
     decoding, and no fetching from hosts we did not name. */
  images: {
    unoptimized: true,
    formats: ['image/webp'],
    remotePatterns: [],
    dangerouslyAllowSVG: false,
  },
};
export default nextConfig;
