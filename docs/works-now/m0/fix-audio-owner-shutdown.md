- Project switches keep the audio device open and retain each open project's
  plug-in instances without rendering background projects. Returning to a project
  keeps it stopped without replaying its previous notes or buffered audio. Browser previews reuse
  the same output. Device changes wait for the old audio owner to finish; a stuck
  shutdown prevents overlapping driver instances and preserves the project.
