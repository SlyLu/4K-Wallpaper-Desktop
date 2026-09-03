interface AppErrorPayload {
  code?: unknown;
  message?: unknown;
}

/** Converts Tauri's structured AppError and ordinary JavaScript failures into readable UI text. */
export function errorMessage(cause: unknown): string {
  if (typeof cause === "string") return cause;
  if (cause instanceof Error && cause.message.trim()) return cause.message;
  if (cause && typeof cause === "object") {
    const payload = cause as AppErrorPayload;
    if (typeof payload.message === "string" && payload.message.trim()) return payload.message;
    try {
      return JSON.stringify(cause);
    } catch {
      // A circular third-party error still receives a stable fallback below.
    }
  }
  return "发生未知错误，请查看运行诊断日志";
}
