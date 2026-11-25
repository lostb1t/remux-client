use anyhow::Result;
use remux_sdks::Endpoint;
use uuid::Uuid;

use crate::media::Media;
use crate::sdks::{
    GetEpisodes, GetItemsQuery, ItemFields, MediaSegmentDto, QueryResult,
};
use crate::server::JellyfinServer;

#[derive(Clone, Copy, Debug)]
pub enum Stage {
    Start,
    Progress,
    Stopped,
}

/// `GET /MediaSegments/{id}`: the intro, credits and so on of an item.
#[derive(Clone)]
pub struct GetMediaSegments {
    pub item_id: Uuid,
}

impl Endpoint for GetMediaSegments {
    type Output = QueryResult<MediaSegmentDto>;

    fn path(&self) -> String {
        format!("/mediasegments/{}", self.item_id)
    }
}

/// The episode after this one, when this is an episode of a show.
pub async fn next_episode(server: &JellyfinServer, episode: &Media) -> Result<Option<Media>> {
    let Some(series_id) = episode.series_id else {
        return Ok(None);
    };
    let res = server
        .api()
        .execute(GetEpisodes {
            series_id,
            query: GetItemsQuery {
                user_id: Some(server.user_id()),
                start_item_id: Some(episode.id),
                limit: Some(2),
                fields: Some(vec![
                    ItemFields::ProviderIds,
                    ItemFields::Genres,
                    ItemFields::Overview,
                    ItemFields::MediaSources,
                ]),
                ..Default::default()
            },
        })
        .await?;
    // The list starts with the episode itself.
    Ok(res
        .items
        .into_iter()
        .find(|item| item.id != episode.id)
        .map(Media::try_from)
        .transpose()?)
}
