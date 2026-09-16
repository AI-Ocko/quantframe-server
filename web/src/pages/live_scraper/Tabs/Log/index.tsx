import api from "@api/index";
import { listen } from "@api/socket";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Button, Chip, Group, Paper, Stack, Text } from "@mantine/core";
import { TauriTypes } from "$types";
import { useEffect, useRef, useState } from "react";

const CAP = 2000;
const TAIL = 500;
const LEVELS = ["TRACE", "DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"];
const DEFAULT_LEVELS = ["INFO", "WARNING", "ERROR", "CRITICAL"];
// How far above the bottom the user has to be before auto-scroll stops following.
const STICK_PX = 40;

function colorOf(level: string) {
  if (level === "WARNING") return "var(--mantine-color-yellow-6)";
  if (level === "ERROR" || level === "CRITICAL") return "var(--mantine-color-red-6)";
  return "inherit";
}

export function LogPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.log.${key}`, context);
  const [lines, setLines] = useState<TauriTypes.LogLine[]>([]);
  const [levels, setLevels] = useState<string[]>(DEFAULT_LEVELS);
  const [paused, setPaused] = useState(false);
  const viewport = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!isActive) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    // Subscribe first, then merge the tail in front of whatever streamed in meanwhile.
    listen<TauriTypes.LogLine>("log", ({ payload }) => setLines((prev) => [...prev, payload].slice(-CAP))).then((off) => {
      if (cancelled) off();
      else unlisten = off;
    });
    api.log
      .tail(TAIL)
      .then((tail) => {
        if (!cancelled) setLines((prev) => [...tail, ...prev].slice(-CAP));
      })
      .catch((error) => console.error("Error loading the log tail:", error));

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [isActive]);

  const scrollToBottom = () => {
    const el = viewport.current;
    if (el) el.scrollTop = el.scrollHeight;
  };

  useEffect(() => {
    if (!paused) scrollToBottom();
  }, [lines, paused]);

  // An unknown level (one without a toggle) is always shown rather than silently dropped.
  const visible = lines.filter((l) => !LEVELS.includes(l.level) || levels.includes(l.level));

  return (
    <Stack mt="md">
      <Group>
        <Text size="sm">{t("levels")}</Text>
        <Chip.Group multiple value={levels} onChange={setLevels}>
          <Group gap="xs">
            {LEVELS.map((level) => (
              <Chip key={level} value={level} size="xs">
                {level}
              </Chip>
            ))}
          </Group>
        </Chip.Group>
        <Button size="xs" variant="default" onClick={() => setLines([])}>
          {t("clear")}
        </Button>
        {paused && (
          <Button
            size="xs"
            onClick={() => {
              setPaused(false);
              scrollToBottom();
            }}
          >
            {t("jump")}
          </Button>
        )}
      </Group>
      <Paper withBorder p="xs">
        <div
          ref={viewport}
          onScroll={(event) => {
            const el = event.currentTarget;
            setPaused(el.scrollHeight - el.scrollTop - el.clientHeight > STICK_PX);
          }}
          style={{ height: "calc(100vh - 320px)", minHeight: 300, overflowY: "auto", fontFamily: "monospace", whiteSpace: "pre-wrap", fontSize: 12 }}
        >
          {visible.length === 0 ? (
            <Text size="sm" c="dimmed">
              {lines.length === 0 ? t("empty") : t("no_match")}
            </Text>
          ) : (
            visible.map((l, index) => (
              <div key={index} style={{ color: colorOf(l.level) }}>
                {l.line}
              </div>
            ))
          )}
        </div>
      </Paper>
    </Stack>
  );
}
