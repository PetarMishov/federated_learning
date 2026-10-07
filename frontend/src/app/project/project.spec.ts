import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router, convertToParamMap, provideRouter } from '@angular/router';
import { BehaviorSubject, of } from 'rxjs';
import { ProjectPage } from './project';

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
    TestBed.inject(HttpTestingController).verify();
    sessionStorage.clear();
  });

  function openDeployments() {
    const fixture = TestBed.createComponent(ProjectPage);
    fixture.detectChanges();
    fixture.nativeElement.querySelector('.project-actions button:nth-child(2)').click();
    fixture.detectChanges();
    return fixture;
  }

  function openMembers() {
    const fixture = TestBed.createComponent(ProjectPage);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
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
});
