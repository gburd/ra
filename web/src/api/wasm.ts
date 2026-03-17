/**
 * WASM database client for browser-based SQL execution.
 *
 * This module provides a bridge to SQLite and DuckDB compiled to
 * WASM, enabling query execution entirely in the browser without
 * a backend server.
 *
 * The actual WASM modules are loaded via web workers to avoid
 * blocking the main thread. This file provides the main-thread
 * API that communicates with those workers.
 */

import type { DatabaseId, ExecuteResponse } from "src/types.ts";

/** Whether WASM databases are available in the current environment. */
export function isWasmAvailable(): boolean {
  return typeof WebAssembly !== "undefined";
}

interface WasmWorkerMessage {
  readonly id: number;
  readonly type: "execute" | "init" | "reset";
  readonly database: DatabaseId;
  readonly sql?: string;
}

interface WasmWorkerResponse {
  readonly id: number;
  readonly success: boolean;
  readonly result?: ExecuteResponse;
  readonly error?: string;
}

type PendingRequest = {
  resolve: (value: ExecuteResponse) => void;
  reject: (reason: Error) => void;
};

/**
 * Client that runs SQL against WASM-compiled databases in a web worker.
 *
 * Usage:
 *   const client = new WasmClient();
 *   await client.init("sqlite");
 *   const result = await client.execute("sqlite", "SELECT 1+1");
 */
export class WasmClient {
  private worker: Worker | null = null;
  private nextId = 1;
  private pending = new Map<number, PendingRequest>();
  private initialized = new Set<DatabaseId>();

  /** Start the web worker. */
  start(): void {
    if (this.worker !== null) return;
    if (!isWasmAvailable()) {
      throw new Error("WebAssembly is not available in this browser");
    }

    this.worker = new Worker(
      new URL("src/workers/wasm-worker.ts", import.meta.url),
      { type: "module" },
    );

    this.worker.onmessage = (event: MessageEvent) => {
      const response = event.data as WasmWorkerResponse;
      const pending = this.pending.get(response.id);
      if (pending === undefined) return;

      this.pending.delete(response.id);
      if (response.success && response.result !== undefined) {
        pending.resolve(response.result);
      } else {
        pending.reject(
          new Error(response.error ?? "Unknown WASM error"),
        );
      }
    };

    this.worker.onerror = (event: ErrorEvent) => {
      for (const [id, pending] of this.pending) {
        pending.reject(new Error(`Worker error: ${event.message}`));
        this.pending.delete(id);
      }
    };
  }

  /** Initialize a WASM database engine. */
  async init(database: DatabaseId): Promise<void> {
    if (this.initialized.has(database)) return;
    await this.send({ type: "init", database });
    this.initialized.add(database);
  }

  /** Execute SQL against a WASM database. */
  async execute(
    database: DatabaseId,
    sql: string,
  ): Promise<ExecuteResponse> {
    if (!this.initialized.has(database)) {
      await this.init(database);
    }
    return this.send({ type: "execute", database, sql });
  }

  /** Reset a WASM database to its initial state. */
  async reset(database: DatabaseId): Promise<ExecuteResponse> {
    return this.send({ type: "reset", database });
  }

  /** Terminate the web worker. */
  terminate(): void {
    if (this.worker === null) return;
    this.worker.terminate();
    this.worker = null;
    this.initialized.clear();
    for (const [, pending] of this.pending) {
      pending.reject(new Error("Worker terminated"));
    }
    this.pending.clear();
  }

  private send(
    msg: Omit<WasmWorkerMessage, "id">,
  ): Promise<ExecuteResponse> {
    if (this.worker === null) {
      this.start();
    }

    const id = this.nextId++;
    const message: WasmWorkerMessage = { ...msg, id };

    return new Promise<ExecuteResponse>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.worker?.postMessage(message);
    });
  }
}
