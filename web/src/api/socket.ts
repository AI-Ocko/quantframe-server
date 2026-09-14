type Handler = (event: { payload: any }) => void;

const handlers = new Map<string, Set<Handler>>();
let socket: WebSocket | undefined;
let retries = 0;

function connect() {
  const protocol = window.location.protocol === "https:" ? "wss" : "ws";
  socket = new WebSocket(`${protocol}://${window.location.host}/ws`);
  socket.onopen = () => {
    retries = 0;
  };
  socket.onmessage = (message) => {
    try {
      const { channel, payload } = JSON.parse(message.data);
      handlers.get(channel)?.forEach((handler) => handler({ payload }));
    } catch (error) {
      console.error("Invalid event frame", error);
    }
  };
  socket.onclose = () => {
    const delay = Math.min(30_000, 1000 * 2 ** retries++);
    setTimeout(connect, delay);
  };
}

/** Same shape as Tauri's `listen`: resolves to an unlisten function. */
export function listen<T = any>(channel: string, handler: (event: { payload: T }) => void): Promise<() => void> {
  if (!socket) connect();
  const set = handlers.get(channel) ?? new Set<Handler>();
  set.add(handler as Handler);
  handlers.set(channel, set);
  return Promise.resolve(() => {
    set.delete(handler as Handler);
  });
}
