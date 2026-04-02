import type { ExplainRequest, ExplainResponse, ErrorResponse } from '../types';

export class ApiError extends Error {
  constructor(public statusCode: number, message: string) {
    super(message);
    this.name = 'ApiError';
  }
}

export async function executeQuery(request: ExplainRequest): Promise<ExplainResponse> {
  const response = await fetch('/api/explain', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    const error = await response.json() as ErrorResponse;
    throw new ApiError(response.status, error.error || 'Unknown error');
  }

  return response.json() as Promise<ExplainResponse>;
}
