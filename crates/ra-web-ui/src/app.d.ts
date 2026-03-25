// See https://svelte.dev/docs/kit/types#app.d.ts

declare global {
  namespace App {}

  interface Window {
    MonacoEnvironment?: {
      getWorker: (moduleId: string, label: string) => Worker;
    };
  }
}

export {};
