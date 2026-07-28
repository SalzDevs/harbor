export type Provider = "gmail" | "outlook";

export type AccountStatus = "stub" | "connected";

export type Account = {
  id: string;
  provider: Provider;
  status: AccountStatus;
  email: string | null;
  displayName: string | null;
  createdAt: number;
};

export type FolderRole =
  | "inbox"
  | "sent"
  | "drafts"
  | "trash"
  | "junk"
  | "archive"
  | "other";

export type Folder = {
  id: string;
  accountId: string;
  imapName: string;
  delimiter: string | null;
  role: FolderRole;
  name: string;
};

export type MessageFlags = {
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  draft: boolean;
};

export type MessageListItem = {
  id: string;
  accountId: string;
  folderId: string;
  uid: number;
  rfcMessageId: string | null;
  subject: string;
  fromAddress: string | null;
  fromName: string | null;
  toList: string | null;
  dateUnix: number;
  size: number | null;
  flags: MessageFlags;
};

export type MessageBody = {
  textPlain: string | null;
  textHtml: string | null;
  textHtmlSafe: string | null;
  fetchedAt: number;
  attachments: AttachmentInfo[];
};

export type AttachmentInfo = {
  section: string;
  filename: string;
  contentType: string;
  size: number | null;
  isInline: boolean;
};

export type MessageDetail = {
  id: string;
  accountId: string;
  folderId: string;
  uid: number;
  rfcMessageId: string | null;
  subject: string;
  fromAddress: string | null;
  fromName: string | null;
  toList: string | null;
  dateUnix: number;
  size: number | null;
  flags: MessageFlags;
  body: MessageBody;
  hasRemoteImages: boolean;
};

export type MessagePage = {
  messages: MessageListItem[];
  total: number;
  offset: number;
  limit: number;
};

export type ConversationListItem = {
  threadRoot: string;
  accountId: string;
  folderId: string;
  messageCount: number;
  unreadCount: number;
  latest: MessageListItem;
};

export type ConversationPage = {
  conversations: ConversationListItem[];
  total: number;
  offset: number;
  limit: number;
};

export type ViewMode = "conversation" | "flat";

export type SearchResult = {
  message: MessageListItem;
  snippet: string;
};

export type SearchPage = {
  results: SearchResult[];
  total: number;
  query: string;
};

export type ComposeKind = "new" | "reply" | "replyAll" | "forward";

export type Draft = {
  id: string;
  accountId: string;
  toList: string;
  ccList: string;
  bccList: string;
  subject: string;
  bodyText: string;
  bodyHtml: string | null;
  inReplyTo: string | null;
  references: string | null;
  signature: string | null;
  updatedAt: number;
};

export type OutboxStatus = "queued" | "sending" | "sent" | "failed";

export type OutboxItem = {
  id: string;
  accountId: string;
  toList: string;
  ccList: string;
  bccList: string;
  subject: string;
  bodyText: string;
  bodyHtml: string | null;
  inReplyTo: string | null;
  references: string | null;
  status: OutboxStatus;
  error: string | null;
  createdAt: number;
};

export type Contact = {
  address: string;
  name: string | null;
  lastSeen: number;
  timesSeen: number;
};

export type NotifyPref = "off" | "unfocused" | "always";

export type FolderSyncProgress = {
  folderId: string;
  fetched: number;
  total: number;
};

export type AppInfo = {
  name: string;
  core: string;
  db: string;
  dataDir: string;
  gmailOauthConfigured: boolean;
  outlookOauthConfigured: boolean;
};

export type ConnectionKind = "online" | "offline" | "reconnecting";

export type ConnectionStatus = {
  kind: ConnectionKind;
  detail: string | null;
  accountId: string | null;
};

export type FolderMailUpdated = {
  folderId: string;
  accountId: string;
};

export type ActionKind = "setFlags" | "move";

export type ActionRecord = {
  id: string;
  kind: ActionKind;
  label: string;
  folderId: string;
  messageId: string;
};
