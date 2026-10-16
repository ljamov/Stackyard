/**
 * A version's notes from the changelog: `- ` lines become a list, `### `
 * lines small headings, anything else a paragraph.
 */
export function ReleaseNotes({ text }: { text: string }) {
  const blocks: { kind: "list" | "heading" | "text"; lines: string[] }[] = [];
  for (const raw of text.split("\n")) {
    const line = raw.trimEnd();
    if (!line.trim()) continue;
    const last = blocks[blocks.length - 1];
    if (/^\s*[-*] /.test(line)) {
      const item = line.replace(/^\s*[-*] /, "");
      if (last?.kind === "list") last.lines.push(item);
      else blocks.push({ kind: "list", lines: [item] });
    } else if (line.startsWith("#")) {
      blocks.push({ kind: "heading", lines: [line.replace(/^#+\s*/, "")] });
    } else if (last?.kind === "list" && /^\s/.test(line)) {
      // A wrapped list item.
      last.lines[last.lines.length - 1] += ` ${line.trim()}`;
    } else if (last?.kind === "text") {
      last.lines.push(line.trim());
    } else {
      blocks.push({ kind: "text", lines: [line.trim()] });
    }
  }
  return (
    <div className="release-notes small">
      {blocks.map((block, i) =>
        block.kind === "list" ? (
          <ul key={i}>
            {block.lines.map((line, j) => (
              <li key={j}>{line}</li>
            ))}
          </ul>
        ) : block.kind === "heading" ? (
          <h3 key={i}>{block.lines[0]}</h3>
        ) : (
          <p key={i}>{block.lines.join(" ")}</p>
        ),
      )}
    </div>
  );
}
