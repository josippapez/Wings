import { basename } from "./format";

/** The files the session edited, each opening its diff, and a button for every change in the working tree. */
export function Files({ files, openDiff }: { files: [string, number][]; openDiff: (path: string | null) => void }) {
  return (
    <>
      <div className="row heading">
        <h2 className="grow">{`Files edited (${files.length})`}</h2>
        <button className="chip" title="Everything changed in the working tree, against HEAD" onClick={() => openDiff(null)}>
          View all changes
        </button>
      </div>
      <ul className="list">
        {files.map(([path, n]) => (
          <li key={path}>
            <button className="toggle row" title={`Diff of ${path}`} onClick={() => openDiff(path)}>
              <span className="grow ellipsis">{basename(path)}</span>
              <span className="muted">{n > 1 ? `${n} edits` : "1 edit"}</span>
            </button>
          </li>
        ))}
      </ul>
    </>
  );
}
