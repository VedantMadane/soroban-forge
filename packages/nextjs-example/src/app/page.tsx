export default function Home() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-center p-24">
      <h1 className="text-4xl font-bold mb-4">Soroban Forge</h1>
      <p className="text-center max-w-xl text-gray-600 dark:text-gray-400">
        Production-ready Soroban smart contracts, reusable Rust libraries, and developer tooling for the Stellar ecosystem.
      </p>
      <div className="mt-8 flex gap-4">
        <a href="https://github.com/Meet-hybrid/soroban-forge" className="text-blue-600 underline">
          View on GitHub
        </a>
        <a
          href="https://github.com/Meet-hybrid/soroban-forge/blob/main/docs/tutorials/getting-started.md"
          className="text-blue-600 underline"
        >
          Get Started
        </a>
      </div>
    </main>
  )
}
