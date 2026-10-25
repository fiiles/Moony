/**
 * What differs between the documents of a policy and of a property: the strings' desktop
 * namespace, the list's query key (the desktop's own, so both screens share one cache and
 * `SyncProvider`'s refresh after a background conversion reaches the phone) and the commands. The
 * section, its add sheet and the viewer take a `DocumentKind` and nothing else.
 */
import type { AttachmentKind, InsuranceDocument, RealEstateDocument } from '@shared/schema';
import { insuranceApi, realEstateApi } from '@/lib/tauri-api';

/** The attachments that are documents (the property's photos have their own gallery). */
export type DocumentKind = Exclude<AttachmentKind, 'photo'>;

export type PhoneDocument = InsuranceDocument | RealEstateDocument;

/** What the add sheet sends: no description on the phone, one type of the caller's list. */
export interface DocumentEntry {
  name: string;
  fileType: string;
}

interface DocumentKindConfig {
  /** The desktop namespace whose document strings the phone reuses. */
  namespace: 'insurance' | 'realEstate';
  /** The section head in that namespace ("Dokumenty"). */
  headKey: string;
  listKey: (ownerId: string) => readonly [string, string];
  list: (ownerId: string) => Promise<PhoneDocument[]>;
  add: (
    ownerId: string,
    bytes: Uint8Array,
    fileName: string,
    entry: DocumentEntry
  ) => Promise<PhoneDocument>;
  remove: (documentId: string) => Promise<void>;
}

export const DOCUMENT_KINDS: Record<DocumentKind, DocumentKindConfig> = {
  'insurance-document': {
    namespace: 'insurance',
    headKey: 'detail.documents',
    listKey: (ownerId) => ['insurance-documents', ownerId],
    list: (ownerId) => insuranceApi.getDocuments(ownerId),
    add: (ownerId, bytes, fileName, entry) =>
      insuranceApi.addDocumentBytes(ownerId, bytes, fileName, entry),
    remove: (documentId) => insuranceApi.deleteDocument(documentId),
  },
  'real-estate-document': {
    namespace: 'realEstate',
    headKey: 'documents.title',
    listKey: (ownerId) => ['real-estate-documents', ownerId],
    list: (ownerId) => realEstateApi.getDocuments(ownerId),
    add: (ownerId, bytes, fileName, entry) =>
      realEstateApi.addDocumentBytes(ownerId, bytes, fileName, entry),
    remove: (documentId) => realEstateApi.deleteDocument(documentId),
  },
};
