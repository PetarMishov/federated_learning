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

  it('defaults the commit to the chosen branch head, allows overrides, and clears stale selections', async () => {
    const fixture = TestBed.createComponent(ProjectPage);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    snapshotListRequest().flush({ snapshots: [], has_more: false });
    http.expectOne('/projects/7/deployments').flush({ deployments: [] });
    fixture.nativeElement.querySelector('.repository-trigger').click();
    http.expectOne('/connectors/github/repositories?page=1').flush({ repositories: [{ id: 42, full_name: 'Org/Repo', web_url: 'https://github.com/Org/Repo' }], has_more: false });
    await fixture.whenStable();
    fixture.nativeElement.querySelector('app-repository-picker [role="option"]').click();
    await fixture.whenStable();
    fixture.nativeElement.querySelector('.branch-trigger').click();
    http.expectOne(req => req.url === '/connectors/github/branches').flush({ branches: [
      { name: 'main', commit_sha: 'a'.repeat(40) },
      { name: 'release', commit_sha: 'b'.repeat(40) },
    ], has_more: false });
    await fixture.whenStable();
    fixture.nativeElement.querySelector('app-branch-picker [role="option"]').click();
    await fixture.whenStable();
    const commit = fixture.nativeElement.querySelector('#snapshot-commit');
    expect(commit.value).toBe('a'.repeat(40));
    commit.value = 'c'.repeat(40);
    commit.dispatchEvent(new Event('input'));
    await fixture.whenStable();
    expect(commit.value).toBe('c'.repeat(40));
    fixture.nativeElement.querySelector('.branch-trigger').click();
    fixture.detectChanges();
    fixture.nativeElement.querySelectorAll('app-branch-picker [role="option"]')[1].click();
    await fixture.whenStable();
    expect(commit.value).toBe('b'.repeat(40));
    fixture.nativeElement.querySelector('.snapshot-sources button:nth-child(2)').click();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('#snapshot-commit').value).toBe('');
    expect(fixture.nativeElement.querySelector('.branch-trigger').disabled).toBe(true);
  });

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

  it('keeps a selected snapshot without a close button or an empty selector option', async () => {
    const fixture = await loadedFiles();
    expect(fixture.nativeElement.querySelector('[aria-label="Close snapshot details"]')).toBeNull();
    expect(fixture.nativeElement.querySelector('#saved-snapshot option[value=""]')).toBeNull();
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

  it('copies the full saved commit and clears feedback when switching snapshots', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Copy saved commit"]').click();
    await fixture.whenStable();
    expect(writeText).toHaveBeenCalledWith(savedSnapshot.git_commit_sha);
    expect(fixture.nativeElement.querySelector('.commit-copy-message').textContent).toBe('Copied!');
    const picker = fixture.nativeElement.querySelector('#saved-snapshot');
    picker.value = '5';
    picker.dispatchEvent(new Event('change'));
    TestBed.inject(HttpTestingController).expectOne('/projects/7/snapshots/5').flush({ ...savedSnapshot, id: 5 });
    flushTree(5, '', rootEntries);
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

  it('shows snapshot files on project load and shows unsupported links as tree entries', async () => {
    const fixture = await loadedFiles();
    const explorer = fixture.nativeElement.querySelector('.file-explorer');
    expect(explorer.textContent).toContain('Snapshot #4');
    expect(explorer.textContent).toContain('README.md');
    expect(explorer.textContent).toContain('src/');
    expect(explorer.querySelector('[aria-label="Open file link"]').textContent).toContain('symlink');
    TestBed.inject(HttpTestingController).expectNone((request) => request.url.endsWith('/file'));
  });

  it('browses nested folders and collapses them while keeping the root visible', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open folder src"]').click();
    flushTree(4, 'src', [{ name: 'space name.txt', path: 'src/space name.txt', kind: 'file' }]);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.file-explorer').textContent).toContain('space name.txt');
    fixture.nativeElement.querySelector('[data-path="src"]').click();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.file-explorer').textContent).toContain('README.md');
  });

  it('fetches a clicked file and displays its contents as editable plain text', async () => {
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
    expect(editor.querySelector('textarea').value).toBe(content);
    expect(editor.querySelector('script')).toBeNull();
    expect(editor.querySelector('textarea').disabled).toBe(false);
  });

  it('cancels a stale file request when another file is clicked', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    const first = fileRequest('README.md');
    fixture.nativeElement.querySelector('[aria-label="Open file train.py"]').click();
    expect(first.cancelled).toBe(true);
    fileRequest('train.py').flush({ path: 'train.py', content: "print('snapshot')" });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.editor textarea').value).toBe("print('snapshot')");
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
    expect(fixture.nativeElement.querySelector('.editor textarea')).toBeNull();
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
    expect(fixture.nativeElement.querySelector('.editor textarea')).toBeNull();
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

  it('keeps the selected snapshot when the deployment request finishes', async () => {
    const fixture = openDeployments([savedSnapshot]);
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    flushTree(4, '', rootEntries);
    http.expectOne('/projects/7/deployments').flush({ deployments: [baseline] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('#saved-snapshot').value).toBe('4');
    http.expectNone('/projects/7/snapshots/4');
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
    expect(fixture.nativeElement.querySelectorAll('#saved-snapshot option').length).toBe(2);
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

  it('keeps edits across files, saves only changes against the base, and selects the new snapshot', async () => {
    const fixture = await loadedFiles();
    const http = TestBed.inject(HttpTestingController);
    const button = () => fixture.nativeElement.querySelector('.snapshot-picker-header button');
    expect(button().disabled).toBe(true);
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    fileRequest('README.md').flush({ path: 'README.md', content: 'original' });
    await fixture.whenStable();
    const edit = (content: string) => {
      const textarea = fixture.nativeElement.querySelector('.editor textarea');
      textarea.value = content;
      textarea.dispatchEvent(new Event('input'));
      fixture.detectChanges();
    };
    edit('changed');
    expect(button().disabled).toBe(false);
    edit('original');
    expect(button().disabled).toBe(true);
    edit('changed');
    fixture.nativeElement.querySelector('[aria-label="Open file train.py"]').click();
    fileRequest('train.py').flush({ path: 'train.py', content: 'print(1)' });
    await fixture.whenStable();
    edit('print(2)');
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    fileRequest('README.md').flush({ path: 'README.md', content: 'original' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('textarea').value).toBe('changed');
    button().click();
    fixture.detectChanges();
    expect(button().disabled).toBe(true);
    expect(fixture.nativeElement.querySelector('textarea').disabled).toBe(true);
    const request = http.expectOne({ method: 'POST', url: '/projects/7/snapshots' });
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    expect(request.request.body).toEqual({ base_snapshot_id: 4, files: [
      { path: 'README.md', content: 'changed' }, { path: 'train.py', content: 'print(2)' },
    ] });
    const created = { ...savedSnapshot, id: 6 };
    request.flush(created);
    http.expectOne('/projects/7/snapshots/6').flush(created);
    flushTree(6, '', rootEntries);
    fileRequest('README.md', 6).flush({ path: 'README.md', content: 'changed' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('#saved-snapshot').value).toBe('6');
    expect(button().disabled).toBe(true);
    expect(fixture.nativeElement.textContent).toContain('Snapshot saved.');
  });

  it('keeps edited contents and allows retry after a failed save', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    fileRequest('README.md').flush({ path: 'README.md', content: 'original' });
    await fixture.whenStable();
    const textarea = fixture.nativeElement.querySelector('textarea');
    textarea.value = 'edited';
    textarea.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    fixture.nativeElement.querySelector('.snapshot-picker-header button').click();
    TestBed.inject(HttpTestingController).expectOne({ method: 'POST', url: '/projects/7/snapshots' })
      .flush('Failed', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    expect(textarea.value).toBe('edited');
    expect(fixture.nativeElement.querySelector('.snapshot-picker-header button').disabled).toBe(false);
    expect(fixture.nativeElement.textContent).toContain('Your edits are kept');
  });

  it('restores unsaved drafts after switching snapshots and preserves CRLF', async () => {
    const fixture = await loadedFiles();
    const http = TestBed.inject(HttpTestingController);
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    fileRequest('README.md').flush({ path: 'README.md', content: 'original\r\n' });
    await fixture.whenStable();
    const textarea = fixture.nativeElement.querySelector('textarea');
    textarea.value = 'edited\n';
    textarea.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    const picker = fixture.nativeElement.querySelector('#saved-snapshot');
    picker.value = '5';
    picker.dispatchEvent(new Event('change'));
    http.expectOne('/projects/7/snapshots/5').flush({ ...savedSnapshot, id: 5 });
    flushTree(5, '', rootEntries);
    await fixture.whenStable();
    picker.value = '4';
    picker.dispatchEvent(new Event('change'));
    http.expectOne('/projects/7/snapshots/4').flush(savedSnapshot);
    flushTree(4, '', rootEntries);
    await fixture.whenStable();
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    fileRequest('README.md').flush({ path: 'README.md', content: 'original\r\n' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('textarea').value).toBe('edited\n');
    fixture.nativeElement.querySelector('.snapshot-picker-header button').click();
    const request = http.expectOne({ method: 'POST', url: '/projects/7/snapshots' });
    expect(request.request.body.files).toEqual([{ path: 'README.md', content: 'edited\r\n' }]);
    request.flush('Unavailable', { status: 500, statusText: 'Error' });
  });

  it('cancels the save response when navigating to a different project', async () => {
    const fixture = await loadedFiles();
    const http = TestBed.inject(HttpTestingController);
    fixture.nativeElement.querySelector('[aria-label="Open file README.md"]').click();
    fileRequest('README.md').flush({ path: 'README.md', content: 'original' });
    await fixture.whenStable();
    const textarea = fixture.nativeElement.querySelector('textarea');
    textarea.value = 'edited';
    textarea.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    fixture.nativeElement.querySelector('.snapshot-picker-header button').click();
    const request = http.expectOne({ method: 'POST', url: '/projects/7/snapshots' });
    params.next(convertToParamMap({ orgId: '3', id: '8' }));
    expect(request.cancelled).toBe(true);
    snapshotListRequest('8').flush({ snapshots: [], has_more: false });
    http.expectOne('/projects/8/deployments').flush({ deployments: [] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-picker-header button').disabled).toBe(true);
    expect(fixture.nativeElement.textContent).not.toContain('unsaved change(s)');
  });

  it('creates a nested draft file, opens it locally and saves it against the selected snapshot', async () => {
    const fixture = await loadedFiles();
    const http = TestBed.inject(HttpTestingController);
    const create = (kind: string, name: string) => {
      contextCreation(fixture, fixture.nativeElement.querySelector('.directory-navigation span').textContent.trim() === '/' ? null : 'models', kind);
      fixture.nativeElement.querySelector('#new-entry-name').value = name;
      fixture.nativeElement.querySelector('.file-create-form').dispatchEvent(new Event('submit', { cancelable: true }));
      fixture.detectChanges();
    };
    create('New folder', 'models');
    expect(fixture.nativeElement.querySelector('.snapshot-picker-header button').disabled).toBe(true);
    fixture.nativeElement.querySelector('[aria-label="Open folder models"]').click();
    await fixture.whenStable();
    http.expectNone((request) => request.url.endsWith('/tree'));
    create('New file', 'train.py');
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('textarea').value).toBe('');
    expect(fixture.nativeElement.querySelector('.snapshot-picker-header button').disabled).toBe(false);
    http.expectNone((request) => request.url.endsWith('/file'));
    const textarea = fixture.nativeElement.querySelector('textarea');
    textarea.value = 'print(42)';
    textarea.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    fixture.nativeElement.querySelector('[data-path="models"]').click();
    await fixture.whenStable();
    fixture.nativeElement.querySelector('[aria-label="Open folder models"]').click();
    await fixture.whenStable();
    fixture.nativeElement.querySelector('[data-path="models/train.py"]').click();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('textarea').value).toBe('print(42)');
    fixture.nativeElement.querySelector('.snapshot-picker-header button').click();
    const request = http.expectOne({ method: 'POST', url: '/projects/7/snapshots' });
    expect(request.request.body).toEqual({ base_snapshot_id: 4, files: [{ path: 'models/train.py', content: 'print(42)' }] });
    const created = { ...savedSnapshot, id: 6 };
    request.flush(created);
    http.expectOne('/projects/7/snapshots/6').flush(created);
    flushTree(6, '', [...rootEntries, { name: 'models', path: 'models', kind: 'directory' }]);
    fileRequest('models/train.py', 6).flush({ path: 'models/train.py', content: 'print(42)' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.snapshot-picker-header button').disabled).toBe(true);
  });

  it('rejects duplicate and unsafe entry names and counts empty new files as changes', async () => {
    const fixture = await loadedFiles();
    contextCreation(fixture, null, 'New file');
    const input = fixture.nativeElement.querySelector('#new-entry-name');
    const form = fixture.nativeElement.querySelector('.file-create-form');
    for (const name of ['../bad', '.git', 'README.md']) {
      input.value = name;
      form.dispatchEvent(new Event('submit', { cancelable: true }));
      fixture.detectChanges();
      expect(form.querySelector('[role="alert"]')).not.toBeNull();
    }
    input.value = 'empty.txt';
    form.dispatchEvent(new Event('submit', { cancelable: true }));
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('[aria-label="Open file empty.txt"]')).not.toBeNull();
    expect(fixture.nativeElement.querySelector('.snapshot-picker-header button').disabled).toBe(false);
  });

  function contextCreation(fixture: ReturnType<typeof TestBed.createComponent<ProjectPage>>, path: string | null, action: string) {
    const target = path === null ? fixture.nativeElement.querySelector('.file-explorer')
      : fixture.nativeElement.querySelector(`[data-path="${path}"]`);
    target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    fixture.detectChanges();
    [...fixture.nativeElement.querySelectorAll('[role="menuitem"]')]
      .find((button: any) => button.textContent.trim() === action).click();
    if (path === null) flushTree(4, '', rootEntries);
    fixture.detectChanges();
  }

  function contextAction(fixture: ReturnType<typeof TestBed.createComponent<ProjectPage>>, path: string, action: string) {
    fixture.nativeElement.querySelector(`[data-path="${path}"]`).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    fixture.detectChanges();
    const menu = fixture.nativeElement.querySelector('[role="menu"]');
    [...menu.querySelectorAll('button')].find((button: any) => button.textContent.trim() === action).click();
    fixture.detectChanges();
  }
  function submitName(fixture: ReturnType<typeof TestBed.createComponent<ProjectPage>>, name: string) {
    fixture.nativeElement.querySelector('#new-entry-name').value = name;
    fixture.nativeElement.querySelector('.file-create-form').dispatchEvent(new Event('submit', { cancelable: true }));
    fixture.detectChanges();
  }

  it('renders nested folders as an expandable tree and collapses without fetching again', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('[data-path="src"]').click();
    flushTree(4, 'src', [{ name: 'main.py', path: 'src/main.py', kind: 'file' }]);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('[data-path="README.md"]')).not.toBeNull();
    expect(fixture.nativeElement.querySelector('[data-path="src"]').getAttribute('aria-expanded')).toBe('true');
    expect(fixture.nativeElement.querySelector('[data-path="src/main.py"]').getAttribute('aria-level')).toBe('2');
    fixture.nativeElement.querySelector('[data-path="src"]').click();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('[data-path="src/main.py"]')).toBeNull();
    expect(fixture.nativeElement.querySelector('[data-path="src"]').getAttribute('aria-expanded')).toBe('false');
  });

  it('renames an edited file and deletes an unopened folder in the next snapshot', async () => {
    const fixture = await loadedFiles();
    const http = TestBed.inject(HttpTestingController);
    fixture.nativeElement.querySelector('[data-path="README.md"]').click();
    fileRequest('README.md').flush({ path: 'README.md', content: 'old' });
    await fixture.whenStable();
    const textarea = fixture.nativeElement.querySelector('textarea');
    textarea.value = 'edited'; textarea.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    contextAction(fixture, 'README.md', 'Rename');
    submitName(fixture, 'notes.md');
    fileRequest('README.md').flush({ path: 'README.md', content: 'old' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('textarea').value).toBe('edited');
    expect(fixture.nativeElement.querySelector('.editor-path').textContent).toBe('notes.md');
    expect(fixture.nativeElement.querySelector('[data-path="README.md"]')).toBeNull();
    contextAction(fixture, 'src', 'Delete');
    expect(fixture.nativeElement.querySelector('[data-path="src"]')).toBeNull();
    fixture.nativeElement.querySelector('.snapshot-picker-header button').click();
    const request = http.expectOne({ method: 'POST', url: '/projects/7/snapshots' });
    expect(request.request.body).toEqual({ base_snapshot_id: 4, files: [{ path: 'notes.md', content: 'edited' }], operations: [
      { kind: 'move', path: 'README.md', to: 'notes.md' }, { kind: 'delete', path: 'src' },
    ] });
    request.flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('[data-path="notes.md"]')).not.toBeNull();
  });

  it('moves a file by dragging into an unloaded folder and saves without downloading its contents', async () => {
    const fixture = await loadedFiles();
    const http = TestBed.inject(HttpTestingController);
    fixture.nativeElement.querySelector('[data-path="README.md"]').dispatchEvent(new Event('dragstart', { bubbles: true }));
    fixture.nativeElement.querySelector('[data-path="src"]').dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }));
    flushTree(4, 'src', []);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('[data-path="README.md"]')).toBeNull();
    expect(fixture.nativeElement.querySelector('[data-path="src/README.md"]')).not.toBeNull();
    http.expectNone((request) => request.url.endsWith('/file'));
    fixture.nativeElement.querySelector('.snapshot-picker-header button').click();
    const request = http.expectOne({ method: 'POST', url: '/projects/7/snapshots' });
    expect(request.request.body).toEqual({ base_snapshot_id: 4, files: [], operations: [{ kind: 'move', path: 'README.md', to: 'src/README.md' }] });
    request.flush('Unavailable', { status: 500, statusText: 'Error' });
  });

  it('rejects rename collisions and dragging a folder into itself', async () => {
    const fixture = await loadedFiles();
    contextAction(fixture, 'README.md', 'Rename');
    submitName(fixture, 'train.py');
    expect(fixture.nativeElement.textContent).toContain('already exists');
    fixture.nativeElement.querySelector('.file-create-form button[type="button"]').click();
    fixture.detectChanges();
    fixture.nativeElement.querySelector('[data-path="src"]').dispatchEvent(new Event('dragstart', { bubbles: true }));
    fixture.nativeElement.querySelector('[data-path="src"]').dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }));
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('cannot be moved into itself');
    expect(fixture.nativeElement.querySelector('.snapshot-picker-header button').disabled).toBe(true);
  });

  it('creates from the context menu and deletes new files without leaving a save operation', async () => {
    const fixture = await loadedFiles();
    fixture.nativeElement.querySelector('.file-explorer').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    fixture.detectChanges();
    [...fixture.nativeElement.querySelectorAll('[role="menuitem"]')].find((button: any) => button.textContent.trim() === 'New file').click();
    flushTree(4, '', rootEntries);
    await fixture.whenStable();
    submitName(fixture, 'temporary.txt');
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('textarea').value).toBe('');
    contextAction(fixture, 'temporary.txt', 'Delete');
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('textarea')).toBeNull();
    expect(fixture.nativeElement.querySelector('[data-path="temporary.txt"]')).toBeNull();
    expect(fixture.nativeElement.querySelector('.snapshot-picker-header button').disabled).toBe(true);
  });

  it('saves a moved base file inside a new draft folder and loads that folder from the new snapshot', async () => {
    const fixture = await loadedFiles();
    const http = TestBed.inject(HttpTestingController);
    contextCreation(fixture, null, 'New folder');
    submitName(fixture, 'models');
    fixture.nativeElement.querySelector('[data-path="README.md"]').dispatchEvent(new Event('dragstart', { bubbles: true }));
    fixture.nativeElement.querySelector('[data-path="models"]').dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }));
    await fixture.whenStable();
    contextAction(fixture, 'models', 'Rename');
    submitName(fixture, 'saved');
    await fixture.whenStable();
    fixture.nativeElement.querySelector('.snapshot-picker-header button').click();
    const request = http.expectOne({ method: 'POST', url: '/projects/7/snapshots' });
    expect(request.request.body.operations).toEqual([
      { kind: 'move', path: 'README.md', to: 'models/README.md' },
      { kind: 'move', path: 'models', to: 'saved' },
    ]);
    const created = { ...savedSnapshot, id: 6 };
    request.flush(created);
    http.expectOne('/projects/7/snapshots/6').flush(created);
    flushTree(6, '', [{ name: 'saved', path: 'saved', kind: 'directory' }]);
    await fixture.whenStable();
    fixture.nativeElement.querySelector('[data-path="saved"]').click();
    flushTree(6, 'saved', [{ name: 'README.md', path: 'saved/README.md', kind: 'file' }]);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('[data-path="saved/README.md"]')).not.toBeNull();
  });

});
