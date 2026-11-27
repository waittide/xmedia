use crate::domain::media::{
    BookFileMeta, BookMetaPatch, ComicFileMeta, ComicMetaPatch, MediaMetadataPatch,
    MediaMetadataPayload, MusicFileMeta, MusicMetaPatch, VideoFileMeta, VideoMetaPatch,
};

fn non_empty_opt(s: &Option<String>) -> bool {
    s.as_ref().is_some_and(|v| !v.trim().is_empty())
}

fn clean_opt_string(raw: String) -> Option<String> {
    (!raw.trim().is_empty()).then(|| raw.trim().to_string())
}

fn clean_string_vec(raw: Vec<String>) -> Vec<String> {
    raw.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn clean_locked_fields(raw: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for item in raw {
        let f = item.trim().to_ascii_lowercase();
        if !f.is_empty() && !deduped.contains(&f) {
            deduped.push(f);
        }
    }
    deduped
}

impl BookFileMeta {
    fn is_field_locked(&self, name: &str) -> bool {
        self.locked_fields
            .iter()
            .any(|f| f.eq_ignore_ascii_case(name))
    }

    pub fn apply_patch(&mut self, patch: BookMetaPatch) {
        if let Some(title) = patch.title
            && !title.trim().is_empty()
        {
            self.title = title.trim().to_string();
        }
        if let Some(authors) = patch.authors {
            self.authors = clean_string_vec(authors);
        }
        if let Some(series) = patch.series {
            self.series = clean_opt_string(series);
        }
        if let Some(subseries) = patch.subseries {
            self.subseries = clean_opt_string(subseries);
        }
        if let Some(idx) = patch.series_index {
            self.series_index = clean_opt_string(idx);
        }
        if let Some(rating) = patch.rating {
            self.rating = (rating.is_finite() && rating >= 0.0).then_some(rating);
        }
        if let Some(desc) = patch.description {
            self.description = clean_opt_string(desc);
        }
        if let Some(pub_name) = patch.publisher {
            self.publisher = clean_opt_string(pub_name);
        }
        if let Some(pub_at) = patch.published_at {
            self.published_at = clean_opt_string(pub_at);
        }
        if let Some(lang) = patch.language {
            self.language = clean_opt_string(lang);
        }
        if let Some(isbn) = patch.isbn {
            self.isbn = clean_opt_string(isbn);
        }
        if let Some(tags) = patch.tags {
            self.tags = clean_string_vec(tags);
        }
        if let Some(locked) = patch.locked_fields {
            self.locked_fields = clean_locked_fields(locked);
        }
    }

    fn filter_patch_for_missing(&self, bp: &mut BookMetaPatch, fallback_stem: &str) {
        let has_tag_block = !self.authors.is_empty()
            || self.publisher.is_some()
            || self.isbn.is_some()
            || !self.tags.is_empty()
            || self.description.is_some();
        if !self.title.trim().is_empty() && (self.title.trim() != fallback_stem || has_tag_block) {
            bp.title = None;
        }
        if !self.authors.is_empty() {
            bp.authors = None;
        }
        if non_empty_opt(&self.series) {
            bp.series = None;
        }
        if non_empty_opt(&self.subseries) {
            bp.subseries = None;
        }
        if non_empty_opt(&self.series_index) {
            bp.series_index = None;
        }
        if self.rating.is_some() {
            bp.rating = None;
        }
        if non_empty_opt(&self.description) {
            bp.description = None;
        }
        if non_empty_opt(&self.publisher) {
            bp.publisher = None;
        }
        if non_empty_opt(&self.published_at) {
            bp.published_at = None;
        }
        if non_empty_opt(&self.language) {
            bp.language = None;
        }
        if non_empty_opt(&self.isbn) {
            bp.isbn = None;
        }
        if !self.tags.is_empty() {
            bp.tags = None;
        }
    }

    fn strip_locked_fields(&self, bp: &mut BookMetaPatch, apply_cover: &mut bool) {
        if self.locked_fields.is_empty() {
            return;
        }
        if self.is_field_locked("title") {
            bp.title = None;
        }
        if self.is_field_locked("authors") {
            bp.authors = None;
        }
        if self.is_field_locked("series") {
            bp.series = None;
        }
        if self.is_field_locked("subseries") {
            bp.subseries = None;
        }
        if self.is_field_locked("series_index") {
            bp.series_index = None;
        }
        if self.is_field_locked("rating") {
            bp.rating = None;
        }
        if self.is_field_locked("isbn") {
            bp.isbn = None;
        }
        if self.is_field_locked("publisher") {
            bp.publisher = None;
        }
        if self.is_field_locked("published_at") {
            bp.published_at = None;
        }
        if self.is_field_locked("language") {
            bp.language = None;
        }
        if self.is_field_locked("tags") {
            bp.tags = None;
        }
        if self.is_field_locked("description") {
            bp.description = None;
        }
        if self.is_field_locked("cover") {
            *apply_cover = false;
        }
    }
}

impl ComicFileMeta {
    pub fn apply_patch(&mut self, cp: ComicMetaPatch) {
        if let Some(series) = cp.series {
            self.series = clean_opt_string(series);
        }
        if let Some(sg) = cp.series_group {
            self.series_group = clean_opt_string(sg);
        }
        if let Some(title) = cp.title {
            self.title = clean_opt_string(title);
        }
        if let Some(num) = cp.number {
            self.number = clean_opt_string(num);
        }
        if let Some(vol) = cp.volume {
            self.volume = (vol.is_finite() && vol >= 0.0).then_some(vol);
        }
        if let Some(sum) = cp.summary {
            self.summary = clean_opt_string(sum);
        }
        if let Some(w) = cp.writers {
            self.writers = clean_string_vec(w);
        }
        if let Some(pen) = cp.pencillers {
            self.pencillers = clean_string_vec(pen);
        }
        if let Some(pub_name) = cp.publisher {
            self.publisher = clean_opt_string(pub_name);
        }
        if let Some(g) = cp.genres {
            self.genres = clean_string_vec(g);
        }
        if let Some(tg) = cp.tags {
            self.tags = clean_string_vec(tg);
        }
        if cp.year.is_some() {
            self.year = cp.year.filter(|&y| y > 0);
        }
        if let Some(lang) = cp.language_iso {
            self.language_iso = clean_opt_string(lang);
        }
        if let Some(dir) = cp.reading_direction
            && !dir.trim().is_empty()
        {
            self.reading_direction = dir.trim().to_ascii_lowercase();
        }
    }

    fn filter_patch_for_missing(&self, cp: &mut ComicMetaPatch) {
        if non_empty_opt(&self.series) {
            cp.series = None;
        }
        if non_empty_opt(&self.series_group) {
            cp.series_group = None;
        }
        if non_empty_opt(&self.title) {
            cp.title = None;
        }
        if non_empty_opt(&self.number) {
            cp.number = None;
        }
        if self.volume.is_some() {
            cp.volume = None;
        }
        if non_empty_opt(&self.summary) {
            cp.summary = None;
        }
        if !self.writers.is_empty() {
            cp.writers = None;
        }
        if !self.pencillers.is_empty() {
            cp.pencillers = None;
        }
        if non_empty_opt(&self.publisher) {
            cp.publisher = None;
        }
        if !self.genres.is_empty() {
            cp.genres = None;
        }
        if !self.tags.is_empty() {
            cp.tags = None;
        }
        if self.year.is_some() {
            cp.year = None;
        }
        if non_empty_opt(&self.language_iso) {
            cp.language_iso = None;
        }
    }
}

impl MusicFileMeta {
    pub fn apply_patch(&mut self, mp: MusicMetaPatch) {
        if let Some(title) = mp.title
            && !title.trim().is_empty()
        {
            self.title = title.trim().to_string();
        }
        if let Some(artist) = mp.artist {
            self.artist = artist.trim().to_string();
        }
        if let Some(album_artist) = mp.album_artist {
            self.album_artist = clean_opt_string(album_artist);
        }
        if let Some(album) = mp.album {
            self.album = album.trim().to_string();
        }
        if mp.track_number.is_some() {
            self.track_number = mp.track_number.filter(|&n| n > 0);
        }
        if mp.disc_number.is_some() {
            self.disc_number = mp.disc_number.filter(|&n| n > 0);
        }
        if mp.year.is_some() {
            self.year = mp.year.filter(|&y| y > 0);
        }
        if let Some(genre) = mp.genre {
            self.genre = clean_opt_string(genre);
        }
        if let Some(comment) = mp.comment {
            self.comment = clean_opt_string(comment);
        }
        if let Some(lyrics) = mp.embedded_lyrics {
            self.embedded_lyrics = clean_opt_string(lyrics);
        }
        if let Some(lrc) = mp.sidecar_lrc {
            self.sidecar_lrc = clean_opt_string(lrc);
        }
    }

    fn filter_patch_for_missing(&self, mp: &mut MusicMetaPatch, fallback_stem: &str) {
        let has_tags = !self.artist.trim().is_empty()
            || !self.album.trim().is_empty()
            || self.track_number.is_some()
            || self.year.is_some();
        if !self.title.trim().is_empty() && (self.title.trim() != fallback_stem || has_tags) {
            mp.title = None;
        }
        if !self.artist.trim().is_empty() {
            mp.artist = None;
        }
        if non_empty_opt(&self.album_artist) {
            mp.album_artist = None;
        }
        if !self.album.trim().is_empty() {
            mp.album = None;
        }
        if self.track_number.is_some() {
            mp.track_number = None;
        }
        if self.disc_number.is_some() {
            mp.disc_number = None;
        }
        if self.year.is_some() {
            mp.year = None;
        }
        if non_empty_opt(&self.genre) {
            mp.genre = None;
        }
        if non_empty_opt(&self.embedded_lyrics) {
            mp.embedded_lyrics = None;
        }
        if non_empty_opt(&self.sidecar_lrc) {
            mp.sidecar_lrc = None;
        }
    }
}

impl VideoFileMeta {
    pub fn apply_patch(&mut self, vp: VideoMetaPatch) {
        if let Some(is_ep) = vp.is_episode {
            self.is_episode = is_ep;
        }
        if let Some(title) = vp.title
            && !title.trim().is_empty()
        {
            self.title = title.trim().to_string();
        }
        if let Some(orig) = vp.original_title {
            self.original_title = clean_opt_string(orig);
        }
        if let Some(show) = vp.show_title {
            self.show_title = clean_opt_string(show);
        }
        if let Some(s) = vp.season_number {
            self.season_number = if s < 0 { None } else { Some(s as u32) };
        }
        if vp.episode_number.is_some() {
            self.episode_number = vp.episode_number.filter(|&n| n > 0);
        }
        if vp.year.is_some() {
            self.year = vp.year.filter(|&y| y > 0);
        }
        if vp.rating.is_some() {
            self.rating = vp.rating.filter(|&r| r > 0.0);
        }
        if let Some(summary) = vp.summary {
            self.summary = clean_opt_string(summary);
        }
        if let Some(genres) = vp.genres {
            self.genres = clean_string_vec(genres);
        }
        if let Some(tags) = vp.tags {
            self.tags = clean_string_vec(tags);
        }
        if let Some(actors) = vp.actors {
            self.actors = clean_string_vec(actors);
        }
        if let Some(directors) = vp.directors {
            self.directors = clean_string_vec(directors);
        }
        if let Some(studio) = vp.studio {
            self.studio = clean_opt_string(studio);
        }
        if let Some(tmdb) = vp.tmdb_id {
            self.tmdb_id = clean_opt_string(tmdb);
        }
        if let Some(bgm) = vp.bangumi_id {
            self.bangumi_id = clean_opt_string(bgm);
        }
        if let Some(set_name) = vp.collection_set {
            self.collection_set = clean_opt_string(set_name);
        }
        if let Some(locks) = vp.locked_fields {
            self.locked_fields = clean_locked_fields(locks);
        }
    }

    fn filter_patch_for_missing(&self, vp: &mut VideoMetaPatch, fallback_stem: &str) {
        if self.has_nfo && !self.title.trim().is_empty() && self.title.trim() != fallback_stem {
            vp.title = None;
        }
        if self.has_nfo {
            vp.is_episode = None;
        }
        if non_empty_opt(&self.original_title) {
            vp.original_title = None;
        }
        if non_empty_opt(&self.show_title) {
            vp.show_title = None;
        }
        if self.season_number.is_some() {
            vp.season_number = None;
        }
        if self.episode_number.is_some() {
            vp.episode_number = None;
        }
        if self.year.is_some() {
            vp.year = None;
        }
        if self.rating.is_some() {
            vp.rating = None;
        }
        if non_empty_opt(&self.summary) {
            vp.summary = None;
        }
        if !self.genres.is_empty() {
            vp.genres = None;
        }
        if !self.tags.is_empty() {
            vp.tags = None;
        }
        if !self.actors.is_empty() {
            vp.actors = None;
        }
        if !self.directors.is_empty() {
            vp.directors = None;
        }
        if non_empty_opt(&self.studio) {
            vp.studio = None;
        }
        if non_empty_opt(&self.tmdb_id) {
            vp.tmdb_id = None;
        }
        if non_empty_opt(&self.bangumi_id) {
            vp.bangumi_id = None;
        }
    }

    fn strip_locked_fields(&self, vp: &mut VideoMetaPatch, apply_cover: &mut bool) {
        strip_video_locked_fields(vp, &self.locked_fields, apply_cover);
    }

    pub fn prepare_and_apply_patch(
        &mut self,
        mut vp: VideoMetaPatch,
        only_fill_missing: bool,
        fallback_stem: &str,
        apply_cover: &mut bool,
    ) {
        if only_fill_missing {
            self.filter_patch_for_missing(&mut vp, fallback_stem);
        }
        self.strip_locked_fields(&mut vp, apply_cover);
        self.apply_patch(vp);
    }
}

fn strip_video_locked_fields(
    vp: &mut VideoMetaPatch,
    locked_fields: &[String],
    apply_cover: &mut bool,
) {
    if locked_fields.is_empty() {
        return;
    }
    let is_locked = |name: &str| locked_fields.iter().any(|f| f.eq_ignore_ascii_case(name));
    if is_locked("title") {
        vp.title = None;
    }
    if is_locked("original_title") {
        vp.original_title = None;
    }
    if is_locked("show_title") {
        vp.show_title = None;
    }
    if is_locked("summary") || is_locked("overview") {
        vp.summary = None;
    }
    if is_locked("genres") {
        vp.genres = None;
    }
    if is_locked("tags") {
        vp.tags = None;
    }
    if is_locked("actors") || is_locked("cast") {
        vp.actors = None;
    }
    if is_locked("directors") {
        vp.directors = None;
    }
    if is_locked("studio") {
        vp.studio = None;
    }
    if is_locked("year") {
        vp.year = None;
    }
    if is_locked("rating") {
        vp.rating = None;
    }
    if is_locked("collection_set") || is_locked("set") {
        vp.collection_set = None;
    }
    if is_locked("cover") || is_locked("poster") {
        *apply_cover = false;
    }
}

impl MediaMetadataPayload {
    pub fn prepare_patch(
        &self,
        patch: &mut MediaMetadataPatch,
        only_fill_missing: bool,
        respect_locks: bool,
        fallback_stem: &str,
        apply_cover: &mut bool,
    ) {
        match (patch, self) {
            (MediaMetadataPatch::Book(bp), Self::Book(b)) => {
                if only_fill_missing {
                    b.filter_patch_for_missing(bp, fallback_stem);
                }
                if respect_locks {
                    b.strip_locked_fields(bp, apply_cover);
                }
            }
            (MediaMetadataPatch::Comic(cp), Self::Comic(c)) => {
                if only_fill_missing {
                    c.filter_patch_for_missing(cp);
                }
            }
            (MediaMetadataPatch::Music(mp), Self::Music(m)) => {
                if only_fill_missing {
                    m.filter_patch_for_missing(mp, fallback_stem);
                }
            }
            (MediaMetadataPatch::Video(vp), Self::Video(v)) => {
                if only_fill_missing {
                    v.filter_patch_for_missing(vp, fallback_stem);
                }
                if respect_locks {
                    v.strip_locked_fields(vp, apply_cover);
                }
            }
            _ => {}
        }
    }

    pub fn to_restore_patch(&self) -> MediaMetadataPatch {
        match self {
            Self::Book(b) => MediaMetadataPatch::Book(BookMetaPatch {
                title: Some(b.title.clone()),
                authors: Some(b.authors.clone()),
                series: Some(b.series.clone().unwrap_or_default()),
                subseries: Some(b.subseries.clone().unwrap_or_default()),
                series_index: Some(b.series_index.clone().unwrap_or_default()),
                rating: Some(b.rating.unwrap_or(-1.0)),
                description: Some(b.description.clone().unwrap_or_default()),
                publisher: Some(b.publisher.clone().unwrap_or_default()),
                published_at: Some(b.published_at.clone().unwrap_or_default()),
                language: Some(b.language.clone().unwrap_or_default()),
                isbn: Some(b.isbn.clone().unwrap_or_default()),
                tags: Some(b.tags.clone()),
                locked_fields: Some(b.locked_fields.clone()),
            }),
            Self::Comic(c) => MediaMetadataPatch::Comic(ComicMetaPatch {
                series: Some(c.series.clone().unwrap_or_default()),
                series_group: Some(c.series_group.clone().unwrap_or_default()),
                title: Some(c.title.clone().unwrap_or_default()),
                number: Some(c.number.clone().unwrap_or_default()),
                volume: Some(c.volume.unwrap_or(-1.0)),
                summary: Some(c.summary.clone().unwrap_or_default()),
                writers: Some(c.writers.clone()),
                pencillers: Some(c.pencillers.clone()),
                publisher: Some(c.publisher.clone().unwrap_or_default()),
                genres: Some(c.genres.clone()),
                tags: Some(c.tags.clone()),
                year: Some(c.year.unwrap_or(0)),
                language_iso: Some(c.language_iso.clone().unwrap_or_default()),
                reading_direction: Some(c.reading_direction.clone()),
            }),
            Self::Music(m) => MediaMetadataPatch::Music(MusicMetaPatch {
                title: Some(m.title.clone()),
                artist: Some(m.artist.clone()),
                album_artist: Some(m.album_artist.clone().unwrap_or_default()),
                album: Some(m.album.clone()),
                track_number: Some(m.track_number.unwrap_or(0)),
                disc_number: Some(m.disc_number.unwrap_or(0)),
                year: Some(m.year.unwrap_or(0)),
                genre: Some(m.genre.clone().unwrap_or_default()),
                comment: Some(m.comment.clone().unwrap_or_default()),
                embedded_lyrics: Some(m.embedded_lyrics.clone().unwrap_or_default()),
                sidecar_lrc: Some(m.sidecar_lrc.clone().unwrap_or_default()),
            }),
            Self::Video(v) => MediaMetadataPatch::Video(VideoMetaPatch {
                is_episode: Some(v.is_episode),
                title: Some(v.title.clone()),
                original_title: Some(v.original_title.clone().unwrap_or_default()),
                show_title: Some(v.show_title.clone().unwrap_or_default()),
                season_number: Some(v.season_number.map(|s| s as i32).unwrap_or(-1)),
                episode_number: Some(v.episode_number.unwrap_or(0)),
                year: Some(v.year.unwrap_or(0)),
                rating: Some(v.rating.unwrap_or(0.0)),
                summary: Some(v.summary.clone().unwrap_or_default()),
                genres: Some(v.genres.clone()),
                tags: Some(v.tags.clone()),
                actors: Some(v.actors.clone()),
                directors: Some(v.directors.clone()),
                studio: Some(v.studio.clone().unwrap_or_default()),
                collection_set: Some(v.collection_set.clone().unwrap_or_default()),
                tmdb_id: Some(v.tmdb_id.clone().unwrap_or_default()),
                bangumi_id: Some(v.bangumi_id.clone().unwrap_or_default()),
                locked_fields: Some(v.locked_fields.clone()),
            }),
        }
    }
}
