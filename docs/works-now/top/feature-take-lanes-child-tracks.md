- **Take lanes as child rows:** take lanes appear when a recording stops, with a mark on the clip
  while it runs; each lane sizes and zooms like a track; Ctrl and a drag paints an area across lanes
  to delete or cut (`take.deleteRange`, `take.cutRange`), pieces move like clips (`take.movePieces`),
  and a piece's lower corners bring back the recording around it (`take.trimPiece`); crossfades
  between parts show on the track and are set by a handle (`take.setCrossfade`), with their default
  in Settings; and a part moved or copied on the track takes its takes along
  ([#1078](https://github.com/suhina-daw/suhina-daw/issues/1078)).
