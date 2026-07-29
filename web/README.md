# web

The React front-end for `chess-engine serve`.

It holds no chess rules. Legality, promotion, check and game end are all
decided by the engine; the page renders whatever `POST /api` returns and sends
back commands. That is why `src/chess.ts` only ever rearranges data the server
already produced.

## Development

```sh
cargo run --release -- serve   # terminal 1: the engine on :8080
npm run dev                    # terminal 2: hot-reloading UI on :5173
```

`vite.config.ts` proxies `/api` from the dev server to the engine, so the UI
reloads on save while still playing real games.

## Building

```sh
npm run build   # -> ../src/web/dist/{index.html,app.js,style.css}
```

The Rust server embeds those three files with `include_str!`, so the release
binary is self-contained. **The build output is committed**, which is what lets
`cargo build` work on a machine with no Node installed. Rebuild and commit it
whenever you change anything under `src/`.

Filenames are pinned (no content hashes) so the server can serve them from three
constant routes.

```sh
npm test        # vitest: pure helpers, plus the App rendered under jsdom
npm run lint
```

## Layout

| File | |
| --- | --- |
| `src/api.ts` | the `GameState` shape and the one `command()` call |
| `src/useGame.ts` | owns the request cycle, including the engine's auto-reply |
| `src/chess.ts` | pure display helpers — FEN to squares, move grouping |
| `src/components/Board.tsx` | squares, pieces, drag and click-to-move |
| `src/components/Panel.tsx` | status, controls, thinking time, move list |
| `src/components/PromotionPicker.tsx` | shown when a target square has several legal moves |
| `src/components/EvalBar.tsx` | white's share of the evaluation |
