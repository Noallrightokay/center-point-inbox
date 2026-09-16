/** @type {import('next').NextConfig} */
const nextConfig = {
  /* RATA renders no <Image>, so the image optimizer is surface with nothing
     behind it — and an unauthenticated endpoint that decodes attacker-supplied
     images is the kind of surface that keeps growing advisories. It carried two
     of them (RCE through AVIF, and unbounded cache growth) on Next 14; the
     Next 16 upgrade fixed those, and turning the endpoint off keeps the next
     one from mattering either.

     formats and remotePatterns are pinned as well, so it stays harmless if a
     future page renders an image and switches the optimizer back on. */
  images: {
    unoptimized: true,
    formats: ['image/webp'],
    remotePatterns: [],
    dangerouslyAllowSVG: false,
  },
};
export default nextConfig;
