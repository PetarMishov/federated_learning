import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router, convertToParamMap, provideRouter } from '@angular/router';
import { BehaviorSubject, of } from 'rxjs';
import { OrganizationPage } from './organization';

describe('Organization projects and members', () => {
  let params: BehaviorSubject<ReturnType<typeof convertToParamMap>>;

  beforeEach(() => {
    sessionStorage.setItem('authToken', 'saved-token');
    params = new BehaviorSubject(convertToParamMap({ id: '1' }));
    const queryParams = convertToParamMap({ name: 'Central Hospital' });
    TestBed.configureTestingModule({
      imports: [OrganizationPage],
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

  it('loads authenticated projects and cancels stale requests when the organization changes', async () => {
    const fixture = TestBed.createComponent(OrganizationPage);
    fixture.detectChanges();
    TestBed.inject(HttpTestingController).expectOne('/organizations/1/members').flush({ members: [] });
    const http = TestBed.inject(HttpTestingController);
    const first = http.expectOne('/organizations/1/projects');
    expect(first.request.headers.get('Authorization')).toBe('Bearer saved-token');
    params.next(convertToParamMap({ id: '2' }));
    expect(first.cancelled).toBe(true);
    http.expectOne('/organizations/2/members').flush({ members: [] });
    http.expectOne('/organizations/2/projects').flush({ projects: [
      { id: 7, org_id: 2, created_by_user_id: 1, name: 'Model training' },
    ] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.projects-panel').textContent).toContain('Model training');
  });

  it('shows errors and retries, then displays the empty state', async () => {
    const fixture = TestBed.createComponent(OrganizationPage);
    fixture.detectChanges();
    TestBed.inject(HttpTestingController).expectOne('/organizations/1/members').flush({ members: [] });
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/organizations/1/projects').flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Could not load projects.');
    fixture.nativeElement.querySelector('.projects-panel button').click();
    http.expectOne('/organizations/1/projects').flush({ projects: [] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.projects-panel').textContent).toContain('No projects to show yet.');
  });

  it('shows organization not found for a 404', async () => {
    const fixture = TestBed.createComponent(OrganizationPage);
    fixture.detectChanges();
    TestBed.inject(HttpTestingController).expectOne('/organizations/1/members').flush({ members: [] });
    TestBed.inject(HttpTestingController).expectOne('/organizations/1/projects')
      .flush('Not found', { status: 404, statusText: 'Not Found' });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Organization not found.');
  });

  it('clears expired credentials and returns to login', async () => {
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = TestBed.createComponent(OrganizationPage);
    fixture.detectChanges();
    TestBed.inject(HttpTestingController).expectOne('/organizations/1/members').flush({ members: [] });
    TestBed.inject(HttpTestingController).expectOne('/organizations/1/projects')
      .flush('Unauthorized', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(navigate).toHaveBeenCalledWith('/login');
  });

  it('loads members with roles and replaces them when changing organizations', async () => {
    const fixture = TestBed.createComponent(OrganizationPage);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/organizations/1/projects').flush({ projects: [] });
    const request = http.expectOne('/organizations/1/members');
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    request.flush({ members: [
      { id: 2, username: 'alice', role_id: 1, role_name: 'Researcher' },
      { id: 3, username: 'bob', role_id: null, role_name: null },
    ] });
    await fixture.whenStable();
    const panel = fixture.nativeElement.querySelector('.members-panel');
    expect(panel.textContent).toContain('alice');
    expect(panel.textContent).toContain('Researcher');
    expect(panel.textContent).toContain('bob');
    params.next(convertToParamMap({ id: '2' }));
    http.expectOne('/organizations/2/projects').flush({ projects: [] });
    http.expectOne('/organizations/2/members').flush({ members: [] });
    await fixture.whenStable();
    expect(panel.textContent).toContain('No members to show yet.');
    expect(panel.textContent).not.toContain('alice');
  });

  it('retries members independently after an error', async () => {
    const fixture = TestBed.createComponent(OrganizationPage);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/organizations/1/projects').flush({ projects: [] });
    http.expectOne('/organizations/1/members').flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Could not load members.');
    fixture.nativeElement.querySelector('.members-panel button').click();
    http.expectNone('/organizations/1/projects');
    http.expectOne('/organizations/1/members').flush({ members: [] });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.members-panel').textContent).toContain('No members to show yet.');
  });

  it('returns to login when members reject the session', async () => {
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = TestBed.createComponent(OrganizationPage);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/organizations/1/projects').flush({ projects: [] });
    http.expectOne('/organizations/1/members').flush('Unauthorized', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(navigate).toHaveBeenCalledWith('/login');
  });
});
