import { useEffect, useRef } from "react";
import { movePairs } from "../chess";

export default function MoveList({ history }: { history: string[] }) {
  const wrap = useRef<HTMLDivElement>(null);

  // Keep the latest move in view.
  useEffect(() => {
    if (wrap.current) wrap.current.scrollTop = wrap.current.scrollHeight;
  }, [history.length]);

  return (
    <div className="history-wrap" ref={wrap}>
      <table className="history">
        <tbody>
          {movePairs(history).map(([number, white, black]) => (
            <tr key={number}>
              <td>{number}.</td>
              <td>{white}</td>
              <td>{black}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
