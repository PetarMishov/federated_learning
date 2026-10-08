import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router, convertToParamMap, provideRouter } from '@angular/router';
import { BehaviorSubject, of } from 'rxjs';
import { ProjectPage } from './project';
import { Snapshot, SnapshotTreeEntry } from '../users-api';

describe('Project sidebars', () => {
  let params: BehaviorSubject<ReturnType<typeof convertToParamMap>>;

  beforeEach(() => {
    sessionStorage.setItem('authToken', 'saved-token');
    params = new BehaviorSubject(convertToParamMap({ orgId: '3', id: '7' }));
    const queryParams = convertToParamMap({ name: 'Training' });
    TestBed.configureTestingModule({
      imports: [ProjectPage],
      providers: [
        provideHttpClient(), provideHttpClientTesting(), provideRouter([]),
        { provide: ActivatedRoute, useValue: {
          paramMap: params.asObservable(),
          queryParamMap: of(queryParams),
          snapshot: { queryParamMap: queryParams },
        } },
      ],
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    TestBed.inject(HttpTestingController).verify();
    sessionStorage.clear();
  });

  function openDeployments(snapshots: Snapshot[] = []) {
    const fixture = TestBed.createComponent(ProjectPage);
    fixture.detectChanges();
    snapshotListRequest().flush({ snapshots, has_more: false });
    fixture.nativeElement.querySelector('.project-actions button:nth-child(2)').click();
    fixture.detectChanges();
    return fixture;
  }

  function openMembers() {
    const fixture = TestBed.createComponent(ProjectPage);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    snapshotListRequest().flush({ snapshots: [], has_more: false });
    http.expectOne('/projects/7/deployments').flush({ deployments: [] });
    http.expectNone('/projects/7/members');
    fixture.nativeElement.querySelector('.project-actions button').click();
    fixture.detectChanges();
    return fixture;
  }

  it('loads members on opening the sidebar and displays their usernames and roles', async () => {
    const fixture = openMembers();
    expect(fixture.nativeElement.querySelector('.drawer').textContent).toContain('Loading members...');
    const request = TestBed.inject(HttpTestingController).expectOne('/projects/7/members');
    expect(request.request.method).toBe('GET');
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    request.flush({ members: [
      { id: 1, username: 'alice', role_id: 4, role_name: 'Coordinator' },
      { id: 2, username: 'owner', role_id: null, role_name: null },
    ] });
    await fixture.whenStable();
    const panel = fixture.nativeElement.querySelector('.drawer');
    expect(panel.textContent).toContain('alice');
    expect(panel.textContent).toContain('Coordinator');
    expect(panel.textContent).toContain('owner');
    expect(panel.querySelectorAll('li').length).toBe(2);
    expect(panel.textContent).not.toContain('null');
  });

  it('cancels stale member requests when changing projects', async () => {
    const fixture = openMembers();
    const http = TestBed.inject(HttpTestingController);
    const first = http.expectOne('/projects/7/members');
    params.next(convertToParamMap({ orgId: '3', id: '8' }));
    expect(first.cancelled).toBe(true);
    http.expectOne('/projects/8/deployments').flush({ deployments: [] });
    snapshotListRequest('8').flush({ snapshots: [], has_more: false });
    http.expectOne('/projects/8/members').flush({ members: [] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.drawer').textContent).toContain('No members to show yet.');
  });

  it('clears previously displayed members when navigating to another project', async () => {
    const fixture = openMembers();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/members').flush({ members: [
      { id: 1, username: 'previous-member', role_id: null, role_name: null },
    ] });
    await fixture.whenStable();
    params.next(convertToParamMap({ orgId: '3', id: '8' }));
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.drawer').textContent).not.toContain('previous-member');
    expect(fixture.nativeElement.querySelector('.drawer').textContent).toContain('Loading members...');
    http.expectOne('/projects/8/deployments').flush({ deployments: [] });
    snapshotListRequest('8').flush({ snapshots: [], has_more: false });
    http.expectOne('/projects/8/members').flush({ members: [] });
    await fixture.whenStable();
  });

  it('retries failed member requests without reloading deployments', async () => {
    const fixture = openMembers();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/members').flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.drawer').textContent).toContain('Could not load members.');
    fixture.nativeElement.querySelector('.drawer [aria-live] button').click();
    http.expectNone('/projects/7/deployments');
    http.expectOne('/projects/7/members').flush({ members: [] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.drawer').textContent).toContain('No members to show yet.');
  });

  it('refreshes members when the sidebar is reopened', async () => {
    const fixture = openMembers();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/members').flush({ members: [] });
    await fixture.whenStable();
    fixture.nativeElement.querySelector('.project-actions button').click();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('.drawer')).toBeNull();
    http.expectNone('/projects/7/members');
    fixture.nativeElement.querySelector('.project-actions button').click();
    http.expectOne('/projects/7/members').flush({ members: [] });
    await fixture.whenStable();
  });

  it('clears expired credentials when loading members and returns to login', async () => {
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = openMembers();
    TestBed.inject(HttpTestingController).expectOne('/projects/7/members')
      .flush('Unauthorized', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(navigate).toHaveBeenCalledWith('/login');
  });

  it('loads authenticated deployments and displays their name, status and date', async () => {
    const fixture = openDeployments();
    expect(fixture.nativeElement.querySelector('.drawer').textContent).toContain('Loading deployments...');
    const request = TestBed.inject(HttpTestingController).expectOne('/projects/7/deployments');
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    request.flush({ deployments: [{
      id: 1, org_id: 3, project_id: 7, snapshot_id: 4, name: 'Baseline training',
      status: 'pending', created_by_user_id: 1, created_at: 1760000000000,
      started_at: null, ended_at: null,
    }] });
    await fixture.whenStable();
    const panel = fixture.nativeElement.querySelector('.drawer');
    expect(panel.textContent).toContain('Baseline training');
    expect(panel.textContent).toContain('pending');
    expect(panel.querySelector('li p').textContent.trim()).not.toBe('');
  });

  it('cancels stale requests when navigating to another project', async () => {
    const fixture = openDeployments();
    const http = TestBed.inject(HttpTestingController);
    const first = http.expectOne('/projects/7/deployments');
    params.next(convertToParamMap({ orgId: '3', id: '8' }));
    expect(first.cancelled).toBe(true);
    http.expectOne('/projects/8/deployments').flush({ deployments: [] });
    snapshotListRequest('8').flush({ snapshots: [], has_more: false });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.drawer').textContent)
      .toContain('No deployments to show yet.');
  });

  it('offers a retry after a failed request', async () => {
    const fixture = openDeployments();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/deployments').flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.drawer').textContent).toContain('Could not load deployments.');
    fixture.nativeElement.querySelector('.drawer [aria-live] button').click();
    http.expectOne('/projects/7/deployments').flush({ deployments: [] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.drawer').textContent)
      .toContain('No deployments to show yet.');
  });

  it('clears expired credentials and returns to login', async () => {
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = openDeployments();
    TestBed.inject(HttpTestingController).expectOne('/projects/7/deployments')
      .flush('Unauthorized', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(navigate).toHaveBeenCalledWith('/login');
  });

  const baseline = {
    id: 1, org_id: 3, project_id: 7, snapshot_id: 4, name: 'Baseline training',
    status: 'pending', created_by_user_id: 1, created_at: 1760000000000,
    started_at: null, ended_at: null,
  };
  const savedSnapshot: Snapshot = {
    id: 4, project_id: 7, created_by_user_id: 1, source: 'local',
    source_branch: null, source_commit_sha: null,
    git_commit_sha: 'a'.repeat(40), created_at: 1790812800000,
  };

  function snapshotListRequest(projectId = '7', offset = 0) {
    return TestBed.inject(HttpTestingController).expectOne((request) =>
      request.url === `/projects/${projectId}/snapshots` && request.params.get('offset') === String(offset));
  }

  function treeRequest(snapshotId = 4, path = '') {
    return TestBed.inject(HttpTestingController).expectOne((request) =>
      request.url === `/projects/7/snapshots/${snapshotId}/tree` && request.params.get('path') === path);
  }

  function fileRequest(path: string, snapshotId = 4) {
    return TestBed.inject(HttpTestingController).expectOne((request) =>
      request.url === `/projects/7/snapshots/${snapshotId}/file` && request.params.get('path') === path);
  }

  function flushTree(snapshotId = 4, path = '', entries: SnapshotTreeEntry[] = []) {
    treeRequest(snapshotId, path).flush({ path, entries });
  }

  async function viewBaselineSnapshot() {
    const fixture = openDeployments([savedSnapshot, { ...savedSnapshot, id: 5, created_at: savedSnapshot.created_at - 1000 }]);
    TestBed.inject(HttpTestingController).expectOne('/projects/7/deployments')
      .flush({ deployments: [baseline, { ...baseline, id: 2, snapshot_id: 5, name: 'Updated training' }] });
    await fixture.whenStable();
    fixture.detectChanges();
    return fixture;
  }

  it('automatically selects the newest saved snapshot', async () => {
    const fixture = await viewBaselineSnapshot();
    expect(fixture.nativeElement.querySelector('.snapshot-details').textContent).toContain('Loading snapshot...');
    const request = TestBed.inject(HttpTestingController).expectOne('/projects/7/snapshots/4');
    expect(request.request.method).toBe('GET');
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    request.flush(savedSnapshot);
    flushTree();
    await fixture.whenStable();
    const panel = fixture.nativeElement.querySelector('.snapshot-details');
    expect(panel.textContent).toContain('Snapshot #4');
    expect(fixture.nativeElement.querySelector('.snapshot-picker').contains(panel)).toBe(true);
    expect(panel.textContent).toContain('Local folder');
    expect(panel.textContent).toContain(savedSnapshot.git_commit_sha);
    expect(panel.textContent).not.toContain('Original branch');
    expect(panel.textContent).not.toContain('null');
    expect(panel.querySelector('dd').textContent.trim()).not.toBe('');
  });

  it('cancels the previous snapshot request when choosing another deployment', async () => {
    const fixture = await viewBaselineSnapshot();
    const http = TestBed.inject(HttpTestingController);
    const first = http.expectOne('/projects/7/snapshots/4');
    fixture.nativeElement.querySelectorAll('.view-snapshot')[1].click();
    expect(first.cancelled).toBe(true);
    http.expectOne('/projects/7/snapshots/5').flush({
      ...savedSnapshot, id: 5, source: 'github', source_branch: 'main', source_commit_sha: 'b'.repeat(40),
    });
    flushTree(5);
    await fixture.whenStable();
    const panel = fixture.nativeElement.querySelector('.snapshot-details');
    expect(panel.textContent).toContain('Snapshot #5');
    expect(panel.textContent).toContain('GitHub');
    expect(panel.textContent).toContain('main');
    expect(panel.textContent).toContain('b'.repeat(40));
  });

  it('cancels snapshot requests and clears details when changing projects', async () => {
    const fixture = await viewBaselineSnapshot();
    const http = TestBed.inject(HttpTestingController);
    const first = http.expectOne('/projects/7/snapshots/4');
    params.next(convertToParamMap({ orgId: '3', id: '8' }));
    expect(first.cancelled).toBe(true);
    http.expectOne('/projects/8/deployments').flush({ deployments: [] });
    snapshotListRequest('8').flush({ snapshots: [], has_more: false });
    http.expectNone('/projects/8/snapshots/4');
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-details')).toBeNull();
  });

  it('closes snapshot details and cancels an in-flight request', async () => {
    const fixture = await viewBaselineSnapshot();
    const request = TestBed.inject(HttpTestingController).expectOne('/projects/7/snapshots/4');
    fixture.nativeElement.querySelector('[aria-label="Close snapshot details"]').click();
    expect(request.cancelled).toBe(true);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-details')).toBeNull();
  });

  it('offers snapshot retry without reloading the deployment list', async () => {
    const fixture = await viewBaselineSnapshot();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/snapshots/4').flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-details').textContent).toContain('Could not load the snapshot.');
    fixture.nativeElement.querySelector('.snapshot-details [aria-live] button').click();
    http.expectNone('/projects/7/deployments');
    http.expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    flushTree();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-details').textContent).toContain(savedSnapshot.git_commit_sha);
  });

  it('explains inaccessible snapshots without retaining previous metadata', async () => {
    const fixture = await viewBaselineSnapshot();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    flushTree();
    await fixture.whenStable();
    fixture.nativeElement.querySelectorAll('.view-snapshot')[1].click();
    http.expectOne('/projects/7/snapshots/5').flush('Not found', { status: 404, statusText: 'Not Found' });
    await fixture.whenStable();
    const panel = fixture.nativeElement.querySelector('.snapshot-details');
    expect(panel.textContent).toContain('Snapshot not found or you no longer have access.');
    expect(panel.textContent).not.toContain(savedSnapshot.git_commit_sha);
  });

  it('returns to login when the snapshot request reports an expired session', async () => {
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = await viewBaselineSnapshot();
    TestBed.inject(HttpTestingController).expectOne('/projects/7/snapshots/4')
      .flush('Unauthorized', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(navigate).toHaveBeenCalledWith('/login');
  });

  const rootEntries: SnapshotTreeEntry[] = [
    { name: 'src', path: 'src', kind: 'directory' },
    { name: 'README.md', path: 'README.md', kind: 'file' },
    { name: 'train.py', path: 'train.py', kind: 'file' },
    { name: 'link', path: 'link', kind: 'symlink' },
  ];

  async function loadedFiles() {
    const fixture = await viewBaselineSnapshot();
    TestBed.inject(HttpTestingController).expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    const request = treeRequest();
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    request.flush({ path: '', entries: rootEntries });
    await fixture.whenStable();
    return fixture;
  }

  it('copies the full saved commit and clears feedback when the snapshot is closed', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Copy saved commit"]').click();
    await fixture.whenStable();
    expect(writeText).toHaveBeenCalledWith(savedSnapshot.git_commit_sha);
    expect(fixture.nativeElement.querySelector('.commit-copy-message').textContent).toBe('Copied!');
    fixture.nativeElement.querySelector('[aria-label="Close snapshot details"]').click();
    fixture.detectChanges();
    const picker = fixture.nativeElement.querySelector('#saved-snapshot');
    picker.value = '4';
    picker.dispatchEvent(new Event('change'));
    TestBed.inject(HttpTestingController).expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    flushTree();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.commit-copy-message').textContent).toBe('');
  });

  it('clears inline copy feedback two seconds after the most recent copy', async () => {
    vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
    const fixture = await loadedFiles();
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    const button = fixture.nativeElement.querySelector('[aria-label="Copy saved commit"]');
    button.click();
    await Promise.resolve();
    fixture.detectChanges();
    const feedback = fixture.nativeElement.querySelector('.commit-copy-message');
    expect(feedback.parentElement).toBe(button.parentElement);
    expect(feedback.textContent).toBe('Copied!');
    vi.advanceTimersByTime(1500);
    button.click();
    await Promise.resolve();
    vi.advanceTimersByTime(1999);
    fixture.detectChanges();
    expect(feedback.textContent).toBe('Copied!');
    vi.advanceTimersByTime(1);
    fixture.detectChanges();
    expect(feedback.textContent).toBe('');
  });

  it('shows copy failure feedback when clipboard access is denied', async () => {
    vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn().mockRejectedValue(new Error('Denied')) } });
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Copy saved commit"]').click();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.commit-copy-message').textContent).toContain('Could not copy.');
  });

  it('shows snapshot files on project load and keeps unsupported links disabled', async () => {
    const fixture = await loadedFiles();
    const explorer = fixture.nativeElement.querySelector('.file-explorer');
    expect(explorer.textContent).toContain('Snapshot #4');
    expect(explorer.textContent).toContain('README.md');
    expect(explorer.textContent).toContain('src/');
    expect(explorer.querySelector('[aria-label="Open file link"]').disabled).toBe(true);
    TestBed.inject(HttpTestingController).expectNone((request) => request.url.endsWith('/file'));
  });

  it('browses nested folders and navigates back to the root', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open folder src"]').click();
    flushTree(4, 'src', [{ name: 'space name.txt', path: 'src/space name.txt', kind: 'file' }]);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.file-explorer').textContent).toContain('space name.txt');
    fixture.nativeElement.querySelector('[aria-label="Go to parent folder"]').click();
    flushTree(4, '', rootEntries);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.file-explorer').textContent).toContain('README.md');
  });

  it('fetches a clicked file and displays its contents as escaped read-only text', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open folder src"]').click();
    flushTree(4, 'src', [{ name: 'space name.txt', path: 'src/space name.txt', kind: 'file' }]);
    await fixture.whenStable();
    fixture.nativeElement.querySelector('[aria-label="Open file space name.txt"]').click();
    const request = fileRequest('src/space name.txt');
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    const content = '<script>alert("test")</script>\n  indented line\n';
    request.flush({ path: 'src/space name.txt', content });
    await fixture.whenStable();
    const editor = fixture.nativeElement.querySelector('.editor');
    expect(editor.querySelector('pre code').textContent).toBe(content);
    expect(editor.querySelector('script')).toBeNull();
    expect(editor.textContent).toContain('Read-only');
  });

  it('cancels a stale file request when another file is clicked', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    const first = fileRequest('README.md');
    fixture.nativeElement.querySelector('[aria-label="Open file train.py"]').click();
    expect(first.cancelled).toBe(true);
    fileRequest('train.py').flush({ path: 'train.py', content: "print('snapshot')" });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.editor pre').textContent).toBe("print('snapshot')");
  });

  it('cancels directory and file requests when switching snapshots', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    const file = fileRequest('README.md');
    fixture.nativeElement.querySelector('[aria-label="Open folder src"]').click();
    const tree = treeRequest(4, 'src');
    fixture.nativeElement.querySelectorAll('.view-snapshot')[1].click();
    expect(file.cancelled).toBe(true);
    expect(tree.cancelled).toBe(true);
    TestBed.inject(HttpTestingController).expectOne('/projects/7/snapshots/5').flush({ ...savedSnapshot, id: 5 });
    flushTree(5);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.editor pre')).toBeNull();
    expect(fixture.nativeElement.querySelector('.file-explorer').textContent).toContain('Snapshot #5');
  });

  it('retries a failed directory request without reloading snapshot metadata', async () => {
    const fixture = await viewBaselineSnapshot();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    treeRequest().flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.file-explorer').textContent).toContain('Could not load files.');
    fixture.nativeElement.querySelector('.file-explorer [aria-live] button').click();
    http.expectNone('/projects/7/snapshots/4');
    flushTree(4, '', rootEntries);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.file-explorer').textContent).toContain('README.md');
  });

  it('retries binary files and shows the size of oversized files with a download placeholder', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    fileRequest('README.md').flush('Binary', { status: 415, statusText: 'Unsupported Media Type' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.editor').textContent).toContain('cannot be displayed as a text file');
    fixture.nativeElement.querySelector('.editor [aria-live] button').click();
    fileRequest('README.md').flush({
      error: 'preview_too_large', size_bytes: 9 * 1024 * 1024, max_preview_bytes: 8 * 1024 * 1024,
    }, { status: 413, statusText: 'Content Too Large' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.editor').textContent).toContain('too large');
    expect(fixture.nativeElement.querySelector('.editor').textContent).toContain('File size: 9 MiB');
    expect(fixture.nativeElement.querySelector('.editor').textContent).toContain('9,437,184 bytes');
    expect(fixture.nativeElement.querySelector('.editor').textContent).toContain('Preview limit: 8 MiB');
    const download = fixture.nativeElement.querySelector('.editor [aria-live] button');
    expect(download.textContent).toBe('Download file');
    expect(download.disabled).toBe(true);
    expect(fixture.nativeElement.querySelector('.editor pre')).toBeNull();
    fixture.nativeElement.querySelector('[aria-label="Open file train.py"]').click();
    fileRequest('train.py').flush({ path: 'train.py', content: '' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.editor').textContent).toContain('This file is empty.');
    expect(fixture.nativeElement.querySelector('.editor').textContent).not.toContain('File size:');
    expect(fixture.nativeElement.querySelector('.editor [aria-live] button')).toBeNull();
  });

  it('clears displayed file contents when navigating to another project', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    fileRequest('README.md').flush({ path: 'README.md', content: 'previous project contents' });
    await fixture.whenStable();
    params.next(convertToParamMap({ orgId: '3', id: '8' }));
    TestBed.inject(HttpTestingController).expectOne('/projects/8/deployments').flush({ deployments: [] });
    snapshotListRequest('8').flush({ snapshots: [], has_more: false });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.editor').textContent).not.toContain('previous project contents');
    expect(fixture.nativeElement.querySelector('.file-explorer').textContent).not.toContain('README.md');
  });

  it('returns to login when a file request reports an expired session', async () => {
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    fileRequest('README.md').flush('Unauthorized', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(navigate).toHaveBeenCalledWith('/login');
  });

  it('selects the newest saved snapshot even with no deployments', async () => {
    const newest = { ...savedSnapshot, id: 9, created_at: savedSnapshot.created_at + 1000 };
    const fixture = TestBed.createComponent(ProjectPage);
    fixture.detectChanges();
    snapshotListRequest().flush({ snapshots: [newest, savedSnapshot], has_more: false });
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    const list = fixture.nativeElement.querySelector('#saved-snapshot');
    http.expectOne('/projects/7/deployments').flush({ deployments: [] });
    http.expectNone('/projects/7/snapshots/4');
    http.expectOne('/projects/7/snapshots/9').flush(newest);
    flushTree(9, '', rootEntries);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-details').textContent).toContain('Snapshot #9');
    expect(fixture.nativeElement.querySelector('.file-explorer').textContent).toContain('README.md');
    expect(list.value).toBe('9');
    expect(fixture.nativeElement.querySelector('.drawer')).toBeNull();
  });

  it('selects a different snapshot directly from the saved snapshot picker', async () => {
    const fixture = await loadedFiles();
    const picker = fixture.nativeElement.querySelector('#saved-snapshot');
    picker.value = '5';
    picker.dispatchEvent(new Event('change'));
    TestBed.inject(HttpTestingController).expectOne('/projects/7/snapshots/5').flush({ ...savedSnapshot, id: 5 });
    flushTree(5, '', rootEntries);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-details').textContent).toContain('Snapshot #5');
    expect(picker.value).toBe('5');
  });

  it('keeps a closed snapshot closed when the deployment request finishes', async () => {
    const fixture = openDeployments([savedSnapshot]);
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    flushTree(4, '', rootEntries);
    await fixture.whenStable();
    fixture.nativeElement.querySelector('[aria-label="Close snapshot details"]').click();
    http.expectOne('/projects/7/deployments').flush({ deployments: [baseline] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-details')).toBeNull();
    expect(fixture.nativeElement.querySelector('#saved-snapshot').value).toBe('');
    http.expectNone('/projects/7/snapshots/4');
    const picker = fixture.nativeElement.querySelector('#saved-snapshot');
    picker.value = '4';
    picker.dispatchEvent(new Event('change'));
    http.expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    flushTree();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-details')).not.toBeNull();
  });

  it('shows an empty snapshot list independently of existing deployments', async () => {
    const fixture = openDeployments();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/deployments').flush({ deployments: [baseline] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-picker').textContent).toContain('No snapshots have been saved yet.');
    expect(fixture.nativeElement.querySelector('.snapshot-details')).toBeNull();
    http.expectNone('/projects/7/snapshots/4');
  });

  it('loads older snapshots without changing the selected version', async () => {
    const fixture = TestBed.createComponent(ProjectPage);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    const request = snapshotListRequest();
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    request.flush({ snapshots: [savedSnapshot], has_more: true });
    http.expectOne('/projects/7/deployments').flush({ deployments: [] });
    http.expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    flushTree();
    await fixture.whenStable();
    const button = Array.from(fixture.nativeElement.querySelectorAll('.snapshot-picker button')).find((element) => element instanceof HTMLButtonElement && element.textContent?.includes('Load older'));
    if (!(button instanceof HTMLButtonElement)) throw new Error('Missing older snapshots button');
    button.click();
    snapshotListRequest('7', 1).flush({ snapshots: [{ ...savedSnapshot, id: 3 }], has_more: false });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelectorAll('#saved-snapshot option').length).toBe(3);
    expect(fixture.nativeElement.querySelector('#saved-snapshot').value).toBe('4');
    http.expectNone('/projects/7/snapshots/3');
  });

  it('retries the snapshot list without depending on the deployment list', async () => {
    const fixture = TestBed.createComponent(ProjectPage);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    snapshotListRequest().flush('Unavailable', { status: 500, statusText: 'Error' });
    http.expectOne('/projects/7/deployments').flush({ deployments: [] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-picker').textContent).toContain('Could not load snapshots.');
    fixture.nativeElement.querySelector('.snapshot-picker [aria-live] button').click();
    snapshotListRequest().flush({ snapshots: [savedSnapshot], has_more: false });
    http.expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    flushTree();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-details').textContent).toContain('Snapshot #4');
    http.expectNone('/projects/7/deployments');
  });

});
