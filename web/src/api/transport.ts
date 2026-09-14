/** Calls a server command. Resolves with the result; rejects with the server's Error JSON, like Tauri's invoke. */
export async function rpcInvoke<T>(command: string, args?: Record<string, any>): Promise<T> {
  const res = await fetch(`/rpc/${command}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify(args ?? {}),
  });
  if (res.status === 401) {
    window.location.href = "/login";
    throw { component: "Rpc", message: "Not signed in" };
  }
  const text = await res.text();
  const body = text ? JSON.parse(text) : null;
  if (!res.ok) throw body ?? { component: "Rpc", message: `Request failed with status ${res.status}` };
  return body as T;
}
