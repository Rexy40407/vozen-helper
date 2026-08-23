/**
 * Public documentation links for the Helper panel.
 *
 * Keep this mapping intentionally boring and data-only: URLs never include
 * session, guild, user, or configuration data. The public docs build checks
 * that every key is present in its generated module index.
 */
const DOCS_ORIGIN = 'https://vozen.org/docs/helper/modules/';

const featureSlugs: Record<string, string> = {
  'community.achievements': 'roles-and-progression/achievements',
  'community.birthdays': 'welcome/birthdays',
  'community.economy': 'community/economy',
  'community.events': 'community/events',
  'community.giveaways': 'community/giveaways',
  'community.leaderboard': 'roles-and-progression/leaderboard',
  'community.levels': 'roles-and-progression/levels',
  'community.role_panels': 'roles-and-progression/role-panels',
  'community.starboard': 'community/starboard',
  'community.suggestions': 'community/suggestions',
  'growth.monetization': 'growth/monetization',
  'insights.stats': 'activity/statistics-channels',
  'management.audit': 'manage/audit',
  'management.custom_commands': 'automate/custom-commands',
  'management.invite_tracker': 'community/invite-tracker',
  'management.moderation': 'manage/moderation',
  'management.nickname': 'management/nickname',
  'management.polls': 'community/polls',
  'management.privacy': 'security/privacy',
  'management.templates': 'manage/templates',
  'management.workflows': 'automate/workflows',
  'protection.anti_raid': 'protect/raid',
  'protection.antiscam': 'protect/scam',
  'protection.antispam': 'protect/spam',
  'protection.join_gate': 'protect/join-gate',
  'social.bluesky': 'alerts/bluesky',
  'social.instagram': 'alerts/instagram',
  'social.kick': 'alerts/kick',
  'social.podcasts': 'alerts/podcasts',
  'social.rss': 'alerts/rss',
  'social.tiktok': 'alerts/tiktok',
  'social.twitch': 'alerts/twitch',
  'social.youtube': 'alerts/youtube',
  'studio.rank_card': 'roles-and-progression/xp-card',
  'support.tickets': 'support/tickets',
  'support.welcome': 'welcome/members',
  'support.welcome_channel': 'welcome/guided-channel',
  'utility.embeds': 'automate/embeds',
  'utility.emojis': 'community/emojis',
  'utility.help': 'manage/help',
  'utility.reminders': 'automate/reminders',
  'utility.search': 'utilities/search',
  'utility.temp_channels': 'community/temporary-channels',
  'web3.crypto_queries': 'web3/crypto-queries',
  'web3.crypto_stats': 'web3/crypto-statistics',
  'web3.gas_tracker': 'web3/gas-tracker',
  'web3.gating': 'web3/wallet-gating',
};

export function docsUrlForFeature(key: string | undefined): string | null {
  const slug = key ? featureSlugs[key] : undefined;
  return slug ? `${DOCS_ORIGIN}${slug}/` : null;
}

export function docsTroubleshootingUrl(slug: string): string {
  return `https://vozen.org/docs/helper/troubleshooting/${slug.replace(/^\/+|\/+$/g, '')}/`;
}

export function docsProviderStatusUrl(): string {
  return 'https://vozen.org/docs/helper/status/providers/';
}
