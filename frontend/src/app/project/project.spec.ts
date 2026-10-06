import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, Router, convertToParamMap, provideRouter } from '@angular/router';
import { BehaviorSubject, of } from 'rxjs';
import { ProjectPage } from './project';

describe('Project deployments', () => {
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
